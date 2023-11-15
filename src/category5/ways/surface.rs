// wl_surface interface
//
// The wayland surface represents an on screen buffer
// this file processes surface events and sends tasks
// to vkcomp
//
// Austin Shafer - 2020
extern crate dakota as dak;
extern crate wayland_server as ws;
use ws::protocol::wl_surface::Request;
use ws::protocol::{wl_buffer, wl_callback, wl_output, wl_region, wl_surface as wlsi};
use ws::Resource;

use super::role::Role;
use super::shm::*;
use super::wl_region::Region;
use crate::category5::atmosphere::{Atmosphere, SurfaceId};
use crate::category5::vkcomp::wm;
use crate::category5::Climate;
use utils::{log, Dmabuf};

use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wlsi::WlSurface, Arc<Mutex<Surface>>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wlsi::WlSurface,
        request: Request,
        data: &Arc<Mutex<Surface>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        let surf = resource.data::<Arc<Mutex<Surface>>>().unwrap();
        surf.lock().unwrap().handle_request(
            state.c_atmos.lock().unwrap().deref_mut(),
            resource,
            data_init,
            request,
        );
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        resource: ws::backend::ObjectId,
        surf: &Arc<Mutex<Surface>>,
    ) {
        surf.lock()
            .unwrap()
            .destroy(state.c_atmos.lock().unwrap().deref_mut());
    }
}

/// Private structure for a wayland surface
///
/// A surface represents a visible area on screen. Desktop organization
/// effects and other transformations are taken care of by a 'shell'
/// interface, not this. A surface will have a buffer attached to it which
/// will be displayed to the client when it is committed.
#[allow(dead_code)]
pub struct Surface {
    pub s_id: SurfaceId, // The id of the window in the renderer
    /// The currently attached buffer. Will be displayed on commit
    /// When the window is created a buffer is not assigned, hence the option
    s_attached_buffer: Option<wl_buffer::WlBuffer>,
    /// the s_attached_buffer is moved here to signify that we can draw
    /// with it.
    pub s_committed_buffer: Option<wl_buffer::WlBuffer>,
    /// Frame callback
    /// This is a power saving feature, we will signal this when the
    /// client should redraw this surface
    pub s_attached_frame_callbacks: Vec<wl_callback::WlCallback>,
    pub s_frame_callbacks: Vec<wl_callback::WlCallback>,
    /// How this surface is being used
    pub s_role: Option<Role>,
    /// Are we currently committing this surface?
    pub s_commit_in_progress: bool,
    /// The opaque region.
    /// vkcomp can optimize displaying this region
    pub s_opaque: Option<Arc<Mutex<Region>>>,
    /// The input region.
    /// Input events will only be delivered if this region is in focus
    pub s_input: Option<Arc<Mutex<Region>>>,
    /// Arrays of damage for this image. This will eventually
    /// be propogated to dakota
    pub s_surf_damage: dak::Damage,
    /// Buffer damage,
    pub s_damage: dak::Damage,
    /// Validates that we cleaned this surf up correctly
    s_is_destroyed: bool,
}

impl Surface {
    // create a new visible surface at coordinates (x,y)
    // from the specified wayland resource
    pub fn new(id: SurfaceId) -> Surface {
        Surface {
            s_id: id,
            s_attached_buffer: None,
            s_committed_buffer: None,
            s_attached_frame_callbacks: Vec::new(),
            s_frame_callbacks: Vec::new(),
            s_role: None,
            s_opaque: None,
            s_input: None,
            s_commit_in_progress: false,
            s_surf_damage: dak::Damage::empty(),
            s_damage: dak::Damage::empty(),
            s_is_destroyed: false,
        }
    }

    fn get_priv_from_region(&self, reg: Option<wl_region::WlRegion>) -> Option<Arc<Mutex<Region>>> {
        match reg {
            Some(r) => Some(r.data::<Arc<Mutex<Region>>>().unwrap().clone()),
            None => None,
        }
    }

    // Handle a request from a client
    //
    // Called by wayland-rs, this function dispatches
    // to the correct handling function.
    #[allow(unused_variables)]
    pub fn handle_request(
        &mut self,
        atmos: &mut Atmosphere,
        surf: &wlsi::WlSurface,
        data_init: &mut ws::DataInit<'_, Climate>,
        req: Request,
    ) {
        match req {
            wlsi::Request::Attach { buffer, x, y } => self.attach(surf, buffer, x, y),
            wlsi::Request::Commit => self.commit(atmos, false),
            wlsi::Request::Damage {
                x,
                y,
                width,
                height,
            } => {
                self.s_surf_damage.add(&dak::Rect::new(x, y, width, height));
            }
            wlsi::Request::DamageBuffer {
                x,
                y,
                width,
                height,
            } => {
                self.s_damage.add(&dak::Rect::new(x, y, width, height));
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
            wlsi::Request::Frame { callback } => {
                let callback_resource = data_init.init(callback, ());
                self.frame(callback_resource)
            }
            // wayland-rs makes us register a destructor
            wlsi::Request::Destroy => self.destroy(atmos),
            // TODO: support variable buffer scaling
            wlsi::Request::SetBufferScale { scale } => {
                if scale != 1 {
                    panic!("Non-1 Buffer scaling is not implemented")
                }
            }
            // TODO: support variable buffer transformation
            wlsi::Request::SetBufferTransform { transform } => {
                if transform.into_result().unwrap() != wl_output::Transform::Normal {
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
        _surf: &wlsi::WlSurface,
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
                .lock()
                .unwrap()
                .should_commit(self, atmos, parent_commit_in_progress)
            {
                log::debug!(
                    "Surf {:?} not committing due to subsurface rules",
                    self.s_id
                );
                return;
            }
        }

        // Before we commit ourselves, we need to
        // commit any subsurfaces available
        self.s_commit_in_progress = true;
        // we need to collect the ids so that atmos won't be borrowed when
        // we recursively call commit below
        let subsurfaces: Vec<_> = atmos.visible_subsurfaces(&self.s_id).collect();
        for id in subsurfaces.iter() {
            let sid = atmos.get_surface_from_id(id);
            if let Some(surf) = sid {
                surf.lock().unwrap().commit(atmos, true);
            }
        }

        // We need to update wm if a new buffer was attached. This includes getting
        // the userdata and sending messages to update window contents.
        //
        // Once the attached buffer is committed, the logic unifies again: the surface
        // size is obtained (either from the new buf or from atmos) and we can start
        // calling down the chain to xdg/wl_subcompositor/wl_shell
        let surf_size = if self.s_attached_buffer.is_some() {
            // now we can commit the attached state
            self.s_committed_buffer = self.s_attached_buffer.take();

            // We need to do different things depending on the
            // type of buffer attached. We detect the type by
            // trying to extract different types of userdat
            let buf = self.s_committed_buffer.as_ref().unwrap();

            // Add tasks that tell the compositor to import this buffer
            // so it is usable in vulkan. Also return the size of the buffer
            // so we can set the surface size
            if let Some(dmabuf) = buf.data::<Arc<Dmabuf>>() {
                atmos.add_wm_task(wm::task::Task::update_window_contents_from_dmabuf(
                    self.s_id.clone(), // ID of the new window
                    dmabuf.clone(),    // fd of the gpu buffer
                    // pass the WlBuffer so it can be released
                    self.s_committed_buffer.as_ref().unwrap().clone(),
                ));
                (dmabuf.db_width as f32, dmabuf.db_height as f32)
            } else if let Some(shm_buf) = buf.data::<Arc<ShmBuffer>>() {
                // ShmBuffer holds the base pointer and an offset, so
                // we need to get the actual pointer, which will be
                // wrapped in a MemImage
                let fb = shm_buf.get_mem_image();

                atmos.add_wm_task(wm::task::Task::update_window_contents_from_mem(
                    self.s_id.clone(), // ID of the new window
                    fb,                // memimage of the contents
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
            *atmos.a_surface_size.get(&self.s_id).unwrap()
        };

        // Commit our frame callbacks
        // move the callback list from attached to the current callback list
        self.s_frame_callbacks
            .extend_from_slice(self.s_attached_frame_callbacks.as_slice());
        self.s_attached_frame_callbacks.clear();
        log::debug!(
            "Surface {:?} New frame callbacks = {:?}",
            self.s_id.get_raw_id(),
            self.s_frame_callbacks
        );
        atmos.a_surface_size.set(&self.s_id, surf_size);

        if !self.s_surf_damage.is_empty() {
            let mut nd = dak::Damage::empty();
            std::mem::swap(&mut self.s_surf_damage, &mut nd);
            log::debug!("Setting surface damage of {:?} to {:?}", self.s_id, nd);
            atmos.a_surface_damage.set(&self.s_id, nd);
        }
        if !self.s_damage.is_empty() {
            let mut nd = dak::Damage::empty();
            std::mem::swap(&mut self.s_damage, &mut nd);
            log::debug!("Setting buffer damage of {:?} to {:?}", self.s_id, nd);
            atmos.a_buffer_damage.set(&self.s_id, nd);
        }

        // Commit any role state before we update window bits
        match &self.s_role {
            Some(Role::xdg_shell_toplevel(_, xs)) => xs.lock().unwrap().commit(&self, atmos),
            Some(Role::xdg_shell_popup(xs)) => xs.lock().unwrap().commit(&self, atmos),
            Some(Role::wl_shell_toplevel) => atmos.a_window_size.set(&self.s_id, surf_size),
            Some(Role::subsurface(ss)) => ss.lock().unwrap().commit(&self, atmos),
            Some(Role::cursor) => {}
            // if we don't have an assigned role, avoid doing
            // any real work
            None => {
                self.s_commit_in_progress = false;
                return;
            }
        }

        // update the surface size of this id so that vkcomp knows what
        // size of buffer it is compositing
        let _win_size = *atmos.a_window_size.get(&self.s_id).unwrap();
        log::debug!(
            "surf {:?}: new sizes are winsize={}x{} surfsize={}x{}",
            self.s_id,
            _win_size.0,
            _win_size.1,
            surf_size.0,
            surf_size.1
        );

        // Make sure to unmark this before returning
        self.s_commit_in_progress = false;
    }

    // Register a frame callback
    //
    // Frame callbacks are a power saving feature, we are going to
    // tell the clients when to update their buffers instead of them
    // guessing. If a client is hidden, then it will not have its
    // callback called, conserving power.
    fn frame(&mut self, callback: wl_callback::WlCallback) {
        // Add this call to our current state, which will
        // be called at the appropriate time
        log::debug!(
            "Surf {:?} attaching frame callback {:?}",
            self.s_id,
            callback
        );
        self.s_attached_frame_callbacks.push(callback);
    }

    // Destroy this surface
    //
    // This must be registered explicitly as the destructor
    // for wayland-rs to call it
    pub fn destroy(&mut self, atmos: &mut Atmosphere) {
        self.s_is_destroyed = true;
        let client = atmos.a_owner.get_clone(&self.s_id).unwrap();
        atmos.free_window_id(&client, &self.s_id);
        atmos.add_wm_task(wm::task::Task::close_window(self.s_id.clone()));
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        if !self.s_is_destroyed {
            panic!("This surface was dropped without being destroyed!");
        }
    }
}

// Add empty definition for wl_callback
#[allow(unused_variables)]
impl ws::Dispatch<wl_callback::WlCallback, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_callback::WlCallback,
        request: wl_callback::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &(),
    ) {
    }
}
