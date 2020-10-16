// wl_surface interface
//
// The wayland surface represents an on screen buffer
// this file processes surface events and sends tasks
// to vkcomp
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::Main;
use ws::protocol::{
    wl_buffer,
    wl_surface as wlsi,
    wl_callback,
    wl_surface,
};

use crate::log;
use crate::category5::utils::{timing::*, logging::LogLevel, atmosphere::*,Dmabuf,WindowId};
use crate::category5::vkcomp::wm;
use super::shm::*;
use super::role::Role;
use super::wl_region::Region;

use std::rc::Rc;
use std::cell::RefCell;

// Private structure for a wayland surface
//
// A surface represents a visible area on screen. Desktop organization
// effects and other transformations are taken care of by a 'shell'
// interface, not this. A surface will have a buffer attached to it which
// will be displayed to the client when it is committed.
#[allow(dead_code)]
pub struct Surface {
    pub s_atmos: Rc<RefCell<Atmosphere>>,
    pub s_surf: Main<wl_surface::WlSurface>,
    pub s_id: WindowId, // The id of the window in the renderer
    // The currently attached buffer. Will be displayed on commit
    // When the window is created a buffer is not assigned, hence the option
    s_attached_buffer: Option<wl_buffer::WlBuffer>,
    // the s_attached_buffer is moved here to signify that we can draw
    // with it.
    pub s_committed_buffer: Option<wl_buffer::WlBuffer>,
    // Frame callback
    // This is a power saving feature, we will signal this when the
    // client should redraw this surface
    pub s_frame_callback: Option<Main<wl_callback::WlCallback>>,
    // How this surface is being used
    pub s_role: Option<Role>,
    // Are we currently committing this surface?
    pub s_commit_in_progress: bool,
}

impl Surface {
    // create a new visible surface at coordinates (x,y)
    // from the specified wayland resource
    pub fn new(atmos: Rc<RefCell<Atmosphere>>,
               surf: Main<wl_surface::WlSurface>,
               id: u32)
               -> Surface
    {
        Surface {
            s_atmos: atmos,
            s_surf: surf,
            s_id: id,
            s_attached_buffer: None,
            s_committed_buffer: None,
            s_frame_callback: None,
            s_role: None,
            s_commit_in_progress: false,
        }
    }

    // Handle a request from a client
    //
    // Called by wayland-rs, this function dispatches
    // to the correct handling function.
    #[allow(unused_variables)]
    pub fn handle_request(&mut self,
                          surf: Main<wlsi::WlSurface>,
                          req: wlsi::Request)
    {
        match req {
            wlsi::Request::Attach { buffer, x, y } =>
                self.attach(surf, buffer, x, y),
            wlsi::Request::Commit =>
                self.commit(),
            // No damage tracking for now
            wlsi::Request::Damage { x, y, width, height } => {},
            wlsi::Request::DamageBuffer { x, y, width, height } => {},
            wlsi::Request::SetOpaqueRegion { region } => {
                let reg = region.as_ref().unwrap()
                    .as_ref().user_data()
                    .get::<Rc<RefCell<Region>>>()
                    .unwrap()
                    .clone();
                let r = reg.borrow();
                log!(LogLevel::debug, "Surface {}: Attaching opaque region {:?}",
                     self.s_id, r);
            },
            wlsi::Request::SetInputRegion { region } => {
                let reg = region.as_ref().unwrap()
                    .as_ref().user_data()
                    .get::<Rc<RefCell<Region>>>()
                    .unwrap()
                    .clone();
                let r = reg.borrow();
                log!(LogLevel::debug, "Surface {}: Attaching input region {:?}",
                     self.s_id, r);
            },
            wlsi::Request::Frame { callback } =>
                self.frame(callback),
            // wayland-rs makes us register a destructor
            wlsi::Request::Destroy =>
                self.destroy(),
            _ => unimplemented!(),
        }
    }

    // attach a wl_buffer to the surface
    //
    // The client crafts a buffer with care, and tells us that it will be
    // backing the surface represented by `resource`. `buffer` will be
    // placed in the private struct that the compositor made.
    fn attach(&mut self,
              _surf: Main<wlsi::WlSurface>,
              buf: Option<wl_buffer::WlBuffer>,
              _x: i32,
              _y: i32)
    {
        self.s_attached_buffer = buf;
    }

    // Commit the current surface configuration to
    // be displayed next frame
    //
    // The commit request tells the compositor that we have
    // fully prepared this surface to be presented to the
    // user. It commits the surface config to vkcomp
    fn commit(&mut self) {
        // If there was no surface attached, do nothing
        if self.s_attached_buffer.is_none() {
            return; // throw error?
        }
        let mut atmos = self.s_atmos.borrow_mut();

        // Before we commit ourselves, we need to
        // commit any subsurfaces available
        self.s_commit_in_progress = true;
        for id in atmos.visible_subsurfaces(self.s_id) {
            atmos.get_surface_from_id(id).map(|surf| {
                surf.borrow_mut()
                    .commit();
            });
        }

        // now we can commit the attached state
        self.s_committed_buffer = self.s_attached_buffer.take();

        // Commit any role state before we do our thing
        match &self.s_role {
            Some(Role::xdg_shell_toplevel(xs)) =>
                xs.borrow_mut().commit(&self, &mut atmos),
            Some(Role::xdg_shell_popup(xs)) =>
                xs.borrow_mut().commit(&self, &mut atmos),
            Some(Role::wl_shell_toplevel) => {},
            Some(Role::subsurface(ss)) =>
                ss.borrow_mut().commit(),
            // if we don't have an assigned role, avoid doing
            // any real work
            None => {
                self.s_commit_in_progress = false;
                return;
            },
        }

        // We need to do different things depending on the
        // type of buffer attached. We detect the type by
        // trying to extract different types of userdat
        let userdata = self.s_committed_buffer
            // this is a bit wonky, we need to get a reference
            // to committed, but it is behind an option
            .as_ref().unwrap()
            // now we can call as_ref on the &WlBuffer
            .as_ref()
            .user_data();

        if let Some(dmabuf) = userdata.get::<Dmabuf>() {
            atmos.add_wm_task(
                wm::task::Task::update_window_contents_from_dmabuf(
                    self.s_id, // ID of the new window
                    *dmabuf, // fd of the gpu buffer
                    // pass the WlBuffer so it can be released
                    self.s_committed_buffer.as_ref().unwrap().clone(),
                )
            );
        } else if let Some(shm_buf) = userdata.get::<ShmBuffer>() {
            // ShmBuffer holds the base pointer and an offset, so
            // we need to get the actual pointer, which will be
            // wrapped in a MemImage
            let fb = shm_buf.get_mem_image();

            atmos.add_wm_task(
                wm::task::Task::update_window_contents_from_mem(
                    self.s_id, // ID of the new window
                    fb, // memimage of the contents
                    // pass the WlBuffer so it can be released
                    self.s_committed_buffer.as_ref().unwrap().clone(),
                    // window dimensions
                    shm_buf.sb_width as usize,
                    shm_buf.sb_height as usize,
                )
            );
        }
        // Make sure to unmark this before returning
        self.s_commit_in_progress = false;
    }

    // Register a frame callback
    //
    // Frame callbacks are a power saving feature, we are going to
    // tell the clients when to update their buffers instead of them
    // guessing. If a client is hidden, then it will not have its
    // callback called, conserving power.
    fn frame(&mut self, callback: Main<wl_callback::WlCallback>) {
        // Add this call to our current state, which will
        // be called at the appropriate time
        self.s_frame_callback = Some(callback);
    }


    // Destroy this surface
    //
    // This must be registered explicitly as the destructor
    // for wayland-rs to call it
    pub fn destroy(&mut self) {
        self.s_atmos.borrow_mut().add_wm_task(
            wm::task::Task::close_window(self.s_id)
        );
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.destroy();
    }
}
