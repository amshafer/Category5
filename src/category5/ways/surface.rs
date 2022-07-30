// wl_surface interface
//
// The wayland surface represents an on screen buffer
// this file processes surface events and sends tasks
// to vkcomp
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::protocol::{wl_buffer, wl_callback, wl_output, wl_region, wl_surface as wlsi, wl_surface};
use ws::Main;

extern crate thundr;
use thundr as th;

use super::role::Role;
use super::shm::*;
use super::wl_region::Region;
use crate::category5::atmosphere::Atmosphere;
use crate::category5::vkcomp::wm;
use utils::{log, Dmabuf, WindowId};

use std::cell::RefCell;
use std::rc::Rc;

/// Private structure for a wayland surface
///
/// A surface represents a visible area on screen. Desktop organization
/// effects and other transformations are taken care of by a 'shell'
/// interface, not this. A surface will have a buffer attached to it which
/// will be displayed to the client when it is committed.
#[allow(dead_code)]
pub struct Surface {
    pub s_atmos: Rc<RefCell<Atmosphere>>,
    pub s_surf: Main<wl_surface::WlSurface>,
    pub s_id: WindowId, // The id of the window in the renderer
    /// The currently attached buffer. Will be displayed on commit
    /// When the window is created a buffer is not assigned, hence the option
    s_attached_buffer: Option<wl_buffer::WlBuffer>,
    /// the s_attached_buffer is moved here to signify that we can draw
    /// with it.
    pub s_committed_buffer: Option<wl_buffer::WlBuffer>,
    /// Frame callback
    /// This is a power saving feature, we will signal this when the
    /// client should redraw this surface
    pub s_attached_frame_callbacks: Vec<Main<wl_callback::WlCallback>>,
    pub s_frame_callbacks: Vec<Main<wl_callback::WlCallback>>,
    /// How this surface is being used
    pub s_role: Option<Role>,
    /// Are we currently committing this surface?
    pub s_commit_in_progress: bool,
    /// The opaque region.
    /// vkcomp can optimize displaying this region
    pub s_opaque: Option<Rc<RefCell<Region>>>,
    /// The input region.
    /// Input events will only be delivered if this region is in focus
    pub s_input: Option<Rc<RefCell<Region>>>,
    /// Arrays of damage for this image. This will eventually
    /// be propogated to thundr
    pub s_surf_damage: th::Damage,
    /// Buffer damage,
    pub s_damage: th::Damage,
}

impl Surface {
    // create a new visible surface at coordinates (x,y)
    // from the specified wayland resource
    pub fn new(
        atmos: Rc<RefCell<Atmosphere>>,
        surf: Main<wl_surface::WlSurface>,
        id: WindowId,
    ) -> Surface {
        Surface {
            s_atmos: atmos,
            s_surf: surf,
            s_id: id,
            s_attached_buffer: None,
            s_committed_buffer: None,
            s_attached_frame_callbacks: Vec::new(),
            s_frame_callbacks: Vec::new(),
            s_role: None,
            s_opaque: None,
            s_input: None,
            s_commit_in_progress: false,
            s_surf_damage: th::Damage::empty(),
            s_damage: th::Damage::empty(),
        }
    }

    fn get_priv_from_region(
        &self,
        reg: Option<wl_region::WlRegion>,
    ) -> Option<Rc<RefCell<Region>>> {
        match reg {
            Some(r) => Some(
                r.as_ref()
                    .user_data()
                    .get::<Rc<RefCell<Region>>>()
                    .unwrap()
                    .clone(),
            ),
            None => None,
        }
    }

    // Handle a request from a client
    //
    // Called by wayland-rs, this function dispatches
    // to the correct handling function.
    #[allow(unused_variables)]
    pub fn handle_request(&mut self, surf: Main<wlsi::WlSurface>, req: wlsi::Request) {
        // we need to clone the atmosphere to make the borrow checker happy. If we don't,
        // then self will be borrowed both here and during the method calls below
        let atmos_cell = self.s_atmos.clone();
        let mut atmos = atmos_cell.borrow_mut();

        match req {
            wlsi::Request::Attach { buffer, x, y } => self.attach(surf, buffer, x, y),
            wlsi::Request::Commit => self.commit(&mut atmos, false),
            wlsi::Request::Damage {
                x,
                y,
                width,
                height,
            } => {
                self.s_surf_damage.add(&th::Rect::new(x, y, width, height));
            }
            wlsi::Request::DamageBuffer {
                x,
                y,
                width,
                height,
            } => {
                self.s_damage.add(&th::Rect::new(x, y, width, height));
            }
            wlsi::Request::SetOpaqueRegion { region } => {
                self.s_opaque = self.get_priv_from_region(region);
                log::debug!(
                    "Surface {:?}: Attaching opaque region {:?}",
                    self.s_id,
                    self.s_opaque
                );
            }
            wlsi::Request::SetInputRegion { region } => {
                self.s_input = self.get_priv_from_region(region);
                log::debug!(
                    "Surface {:?}: Attaching input region {:?}",
                    self.s_id,
                    self.s_input
                );
            }
            wlsi::Request::Frame { callback } => self.frame(callback),
            // wayland-rs makes us register a destructor
            wlsi::Request::Destroy => self.destroy(&mut atmos),
            // TODO: support variable buffer scaling
            wlsi::Request::SetBufferScale { scale } => {
                if scale != 1 {
                    panic!("Non-1 Buffer scaling is not implemented")
                }
            }
            // TODO: support variable buffer transformation
            wlsi::Request::SetBufferTransform { transform } => {
                if transform != wl_output::Transform::Normal {
                    panic!("Non-normal Buffer transformation is not implemented");
                }
            }
            _ => unimplemented!(),
        }
    }

    // attach a wl_buffer to the surface
    //
    // The client crafts a buffer with care, and tells us that it will be
    // backing the surface represented by `resource`. `buffer` will be
    // placed in the private struct that the compositor made.
    fn attach(
        &mut self,
        _surf: Main<wlsi::WlSurface>,
        buf: Option<wl_buffer::WlBuffer>,
        _x: i32,
        _y: i32,
    ) {
        self.s_attached_buffer = buf;
    }

    /// Commit the current surface configuration to
    /// be displayed next frame
    ///
    /// The commit request tells the compositor that we have
    /// fully prepared this surface to be presented to the
    /// user. It commits the surface config to vkcomp
    ///
    /// Atmosphere is passed in since committing one surface
    /// will recursively call commit on the subsurfaces, and
    /// we need to avoid a refcell panic.
    fn commit(&mut self, atmos: &mut Atmosphere, parent_commit_in_progress: bool) {
        if let Some(Role::subsurface(ss)) = &self.s_role {
            if !ss
                .borrow_mut()
                .should_commit(self, atmos, parent_commit_in_progress)
            {
                return;
            }
        }

        // Before we commit ourselves, we need to
        // commit any subsurfaces available
        self.s_commit_in_progress = true;
        // we need to collect the ids so that atmos won't be borrowed when
        // we recursively call commit below
        let subsurfaces: Vec<_> = atmos.visible_subsurfaces(self.s_id).collect();
        for id in subsurfaces.iter() {
            let sid = atmos.get_surface_from_id(*id);
            if let Some(surf) = sid {
                surf.borrow_mut().commit(atmos, true);
            }
        }

        // We need to update wm if a new buffer was attached. This includes getting
        // the userdata and sending messages to update window contents.
        //
        // Once the attached buffer is committed, the logic unifies again: the surface
        // size is obtained (either from the new buf or from atmos) and we can start
        // calling down the chain to xdg/wl_subcompositor/wl_shell
        let surf_size = if let Some(_) = self.s_attached_buffer.as_ref() {
            // now we can commit the attached state
            self.s_committed_buffer = self.s_attached_buffer.take();

            // We need to do different things depending on the
            // type of buffer attached. We detect the type by
            // trying to extract different types of userdat
            let userdata = self
                .s_committed_buffer
                // this is a bit wonky, we need to get a reference
                // to committed, but it is behind an option
                .as_ref()
                .unwrap()
                // now we can call as_ref on the &WlBuffer
                .as_ref()
                .user_data();

            // Add tasks that tell the compositor to import this buffer
            // so it is usable in vulkan. Also return the size of the buffer
            // so we can set the surface size
            if let Some(dmabuf) = userdata.get::<Dmabuf>() {
                atmos.add_wm_task(wm::task::Task::update_window_contents_from_dmabuf(
                    self.s_id, // ID of the new window
                    *dmabuf,   // fd of the gpu buffer
                    // pass the WlBuffer so it can be released
                    self.s_committed_buffer.as_ref().unwrap().clone(),
                ));
                (dmabuf.db_width as f32, dmabuf.db_height as f32)
            } else if let Some(shm_buf) = userdata.get::<ShmBuffer>() {
                // ShmBuffer holds the base pointer and an offset, so
                // we need to get the actual pointer, which will be
                // wrapped in a MemImage
                let fb = shm_buf.get_mem_image();

                atmos.add_wm_task(wm::task::Task::update_window_contents_from_mem(
                    self.s_id, // ID of the new window
                    fb,        // memimage of the contents
                    // pass the WlBuffer so it can be released
                    self.s_committed_buffer.as_ref().unwrap().clone(),
                    // window dimensions
                    shm_buf.sb_width as usize,
                    shm_buf.sb_height as usize,
                ));
                (shm_buf.sb_width as f32, shm_buf.sb_height as f32)
            } else {
                panic!("Could not find dmabuf or shmbuf private data for wl_buffer");
            }
        } else {
            atmos.get_surface_size(self.s_id)
        };

        // Commit any role state before we do our thing
        match &self.s_role {
            Some(Role::xdg_shell_toplevel(xs)) => xs.borrow_mut().commit(&self, atmos),
            Some(Role::xdg_shell_popup(xs)) => xs.borrow_mut().commit(&self, atmos),
            Some(Role::wl_shell_toplevel) => {
                atmos.set_window_size(self.s_id, surf_size.0, surf_size.1)
            }
            Some(Role::subsurface(ss)) => ss.borrow_mut().commit(&self, atmos),
            // if we don't have an assigned role, avoid doing
            // any real work
            None => {
                self.s_commit_in_progress = false;
                return;
            }
        }

        // Commit our frame callbacks
        // move the callback list from attached to the current callback list
        self.s_frame_callbacks = std::mem::take(&mut self.s_attached_frame_callbacks);

        // update the surface size of this id so that vkcomp knows what
        // size of buffer it is compositing
        let _win_size = atmos.get_window_size(self.s_id);
        log::debug!(
            "surf {:?}: new sizes are winsize={}x{} surfsize={}x{}",
            self.s_id,
            _win_size.0,
            _win_size.1,
            surf_size.0,
            surf_size.1
        );
        atmos.set_surface_size(self.s_id, surf_size.0, surf_size.1);

        if !self.s_surf_damage.is_empty() {
            let mut nd = th::Damage::empty();
            std::mem::swap(&mut self.s_surf_damage, &mut nd);
            log::debug!("Setting surface damage of {:?} to {:?}", self.s_id, nd);
            atmos.set_surface_damage(self.s_id, nd);
        }
        if !self.s_damage.is_empty() {
            let mut nd = th::Damage::empty();
            std::mem::swap(&mut self.s_damage, &mut nd);
            log::debug!("Setting buffer damage of {:?} to {:?}", self.s_id, nd);
            atmos.set_buffer_damage(self.s_id, nd);
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
        self.s_attached_frame_callbacks.push(callback);
    }

    // Destroy this surface
    //
    // This must be registered explicitly as the destructor
    // for wayland-rs to call it
    pub fn destroy(&mut self, atmos: &mut Atmosphere) {
        let client = atmos.get_owner(self.s_id);
        atmos.free_window_id(client, self.s_id);
        atmos.add_wm_task(wm::task::Task::close_window(self.s_id));
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        let atmos_rc = self.s_atmos.clone();
        let mut atmos = atmos_rc.borrow_mut();
        self.destroy(&mut atmos);
    }
}
