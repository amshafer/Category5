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
use super::wl_region::Region;
use super::{shm::*, wl_subcompositor::SubSurfaceState};
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

/// State of a wl_surface
///
/// wl_surface works by receiving a number of requests and setting some
/// double buffered state as a result of it. This state is then atomically
/// applied at commit time. There may be some reasons that we want to delay
/// applying state, such as a synchronized subsurface whose parent has not
/// yet committed, or when we are waiting for explicit synchronization.
///
/// This struct holds all surface state and provides a way to commit
/// it when requested. The surface protocol handling code will populate
/// this and commit it at the appropriate time.
pub struct CommitState {
    /// ECS id of the surface this represents
    pub cs_id: SurfaceId,
    /// The current wl_buffer defining the contents of this surface.
    pub cs_buffer: Option<wl_buffer::WlBuffer>,
    /// Frame callback
    /// This is a power saving feature, we will signal this when the
    /// client should redraw this surface
    pub cs_frame_callbacks: Vec<wl_callback::WlCallback>,
    /// The opaque region.
    /// vkcomp can optimize displaying this region
    pub cs_opaque: Option<Arc<Mutex<Region>>>,
    /// The input region.
    /// Input events will only be delivered if this region is in focus
    pub cs_input: Option<Arc<Mutex<Region>>>,
    /// Arrays of damage for this image. This will eventually
    /// be propogated to dakota
    pub cs_surf_damage: dak::Damage,
    /// Damage in buffer coordinates.
    pub cs_damage: dak::Damage,
    /// Surface position change from attach/offset
    cs_attached_xy: Option<(i32, i32)>,

    /// State programmed by wl_subcompositor
    pub cs_subsurf_state: SubSurfaceState,

    /// Child CommitStates which are dependent on this before they
    /// can be committed. These are usually synchronized subsurface
    /// commits that are pending.
    pub cs_children: Vec<CommitState>,
}

impl CommitState {
    /// Initializes an empty state object for this surface id
    fn new(id: SurfaceId) -> Self {
        Self {
            cs_id: id.clone(),
            cs_buffer: None,
            cs_frame_callbacks: Vec::with_capacity(1),
            cs_opaque: None,
            cs_input: None,
            cs_surf_damage: dak::Damage::empty(),
            cs_damage: dak::Damage::empty(),
            cs_attached_xy: None,
            cs_subsurf_state: SubSurfaceState::new(id),
            cs_children: Vec::with_capacity(0),
        }
    }

    /// Clone a copy of this state and reset it
    ///
    /// This clones a new copy of this state as-is, but clears out
    /// any fields that don't persist.
    pub fn clone_refresh(&mut self) -> Self {
        let mut frame_callbacks = Vec::with_capacity(1);
        std::mem::swap(&mut frame_callbacks, &mut self.cs_frame_callbacks);

        let mut children = Vec::with_capacity(0);
        std::mem::swap(&mut children, &mut self.cs_children);

        let mut surf_damage = dak::Damage::empty();
        std::mem::swap(&mut surf_damage, &mut self.cs_surf_damage);
        let mut damage = dak::Damage::empty();
        std::mem::swap(&mut damage, &mut self.cs_damage);

        Self {
            cs_id: self.cs_id.clone(),
            cs_buffer: self.cs_buffer.clone(),
            cs_frame_callbacks: frame_callbacks,
            cs_opaque: self.cs_opaque.clone(),
            cs_input: self.cs_input.clone(),
            cs_surf_damage: surf_damage,
            cs_damage: damage,
            cs_attached_xy: self.cs_attached_xy.take(),
            cs_subsurf_state: self.cs_subsurf_state.clone_refresh(),
            cs_children: children,
        }
    }

    /// Commit this state
    ///
    /// This actually does all the work to apply the state info to
    /// the system, resetting the state in the process. Any child states
    /// will also be applied at this time.
    pub fn commit(&mut self, atmos: &mut Atmosphere) {
        log::debug!("Committing state for surface {:?}", self.cs_id.get_raw_id());

        // ----- Update our surface size -----
        // We need to update wm if a new buffer was attached. This includes getting
        // the userdata and sending messages to update window contents.
        //
        // Once the attached buffer is committed, the logic unifies again: the surface
        // size is obtained (either from the new buf or from atmos) and we can start
        // calling down the chain to xdg/wl_subcompositor/wl_shell
        let surf_size = if let Some(buf) = self.cs_buffer.take() {
            // Add tasks that tell the compositor to import this buffer
            // so it is usable in vulkan. Also return the size of the buffer
            // so we can set the surface size
            if let Some(dmabuf) = buf.data::<Arc<Dmabuf>>() {
                atmos.add_wm_task(wm::task::Task::update_window_contents_from_dmabuf(
                    self.cs_id.clone(), // ID of the new window
                    dmabuf.clone(),     // fd of the gpu buffer
                    // pass the WlBuffer so it can be released
                    buf.clone(),
                ));
                (dmabuf.db_width as f32, dmabuf.db_height as f32)
            } else if let Some(shm_buf) = buf.data::<Arc<ShmBuffer>>() {
                // ShmBuffer holds the base pointer and an offset, so
                // we need to get the actual pointer, which will be
                // wrapped in a MemImage
                let fb = shm_buf.get_mem_image();

                atmos.add_wm_task(wm::task::Task::update_window_contents_from_mem(
                    self.cs_id.clone(), // ID of the new window
                    fb,                 // memimage of the contents
                    // pass the WlBuffer so it can be released
                    buf.clone(),
                    // window dimensions
                    shm_buf.sb_width as usize,
                    shm_buf.sb_height as usize,
                ));
                (shm_buf.sb_width as f32, shm_buf.sb_height as f32)
            } else {
                panic!("Could not find dmabuf or shmbuf private data for wl_buffer");
            }
        } else {
            *atmos.a_surface_size.get(&self.cs_id).unwrap()
        };
        atmos.a_surface_size.set(&self.cs_id, surf_size);

        // ----- Commit our frame callbacks -----
        if self.cs_frame_callbacks.len() > 0 {
            log::debug!(
                "Surface {:?} New frame callbacks = {:?}",
                self.cs_id.get_raw_id(),
                self.cs_frame_callbacks
            );

            if atmos.a_frame_callbacks.get_mut(&self.cs_id).is_none() {
                atmos
                    .a_frame_callbacks
                    .set(&self.cs_id, Vec::with_capacity(1));
            }

            // Extend the existing list of callbacks to signal
            let mut cbs = atmos.a_frame_callbacks.get_mut(&self.cs_id).unwrap();
            cbs.extend_from_slice(self.cs_frame_callbacks.as_slice());
            self.cs_frame_callbacks.clear();
        }

        // ------ Update damage regions -----
        if !self.cs_surf_damage.is_empty() {
            let mut nd = dak::Damage::empty();
            std::mem::swap(&mut self.cs_surf_damage, &mut nd);
            log::debug!("Setting surface damage of {:?} to {:?}", self.cs_id, nd);
            atmos.a_surface_damage.set(&self.cs_id, nd);
        }
        if !self.cs_damage.is_empty() {
            let mut nd = dak::Damage::empty();
            std::mem::swap(&mut self.cs_damage, &mut nd);
            log::debug!("Setting buffer damage of {:?} to {:?}", self.cs_id, nd);
            atmos.a_buffer_damage.set(&self.cs_id, nd);
        }

        // ------ Update input/opaque regions -----
        if let Some(reg) = self.cs_opaque.take() {
            log::debug!("Setting opaque region of {:?} to {:?}", self.cs_id, reg);
            atmos.a_opaque_region.set(&self.cs_id, reg);
        }
        if let Some(reg) = self.cs_input.take() {
            log::debug!("Setting input region of {:?} to {:?}", self.cs_id, reg);
            atmos.a_input_region.set(&self.cs_id, reg);
        }

        // ----- Move our surfaces position if requested -----
        //
        // The surface attach and offset functions allow for changing the top
        // left corner of the surface.
        if let Some((x, y)) = self.cs_attached_xy.take() {
            log::debug!("Surface requested move of {:?}", (x, y));
            {
                let mut pos = atmos.a_surface_pos.get_mut(&self.cs_id).unwrap();
                pos.0 += x as f32;
                pos.1 += y as f32;
            }
            // According to the spec we subtract the change from the cursor
            // hotspot to adjust the cursor position
            //
            // Only update this if we are the surface in focus, otherwise this will
            // offset the cursor for another surface
            if atmos.get_cursor_surface().map(|e| e.get_raw_id()) == Some(self.cs_id.get_raw_id()) {
                let hotspot = atmos.get_cursor_hotspot();
                log::debug!("original hotspot {:?}", hotspot);
                atmos.set_cursor_hotspot((hotspot.0 - x, hotspot.1 - y));
                log::debug!("new hotspot {:?}", (hotspot.0 - x, hotspot.1 - y,));
            }
        }

        let _win_size = atmos.a_window_size.get(&self.cs_id).map(|ws| *ws);
        log::debug!(
            "surf {:?}: new sizes are winsize={:?} surfsize={:?}",
            self.cs_id,
            _win_size,
            surf_size,
        );

        // ----- now commit any child protocols state -----
        self.cs_subsurf_state.commit(atmos);

        // ----- commit all of the pending child commits -----
        for mut cs in self.cs_children.drain(0..) {
            cs.commit(atmos);
        }
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
    /// Our current pending state
    pub s_state: CommitState,
    /// How this surface is being used
    pub s_role: Option<Role>,
    /// Validates that we cleaned this surf up correctly
    s_is_destroyed: bool,
}

impl Surface {
    // create a new visible surface at coordinates (x,y)
    // from the specified wayland resource
    pub fn new(id: SurfaceId) -> Surface {
        Surface {
            s_id: id.clone(),
            s_role: None,
            s_is_destroyed: false,
            s_state: CommitState::new(id),
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
            wlsi::Request::Commit => self.commit(atmos),
            wlsi::Request::Damage {
                x,
                y,
                width,
                height,
            } => {
                self.s_state
                    .cs_surf_damage
                    .add(&dak::Rect::new(x, y, width, height));
            }
            wlsi::Request::DamageBuffer {
                x,
                y,
                width,
                height,
            } => {
                self.s_state
                    .cs_damage
                    .add(&dak::Rect::new(x, y, width, height));
            }
            wlsi::Request::SetOpaqueRegion { region } => {
                self.s_state.cs_opaque = self.get_priv_from_region(region);
                log::debug!(
                    "Surface {:?}: Attaching opaque region {:?}",
                    self.s_id,
                    self.s_state.cs_opaque
                );
            }
            wlsi::Request::SetInputRegion { region } => {
                self.s_state.cs_input = self.get_priv_from_region(region);
                log::debug!(
                    "Surface {:?}: Attaching input region {:?}",
                    self.s_id,
                    self.s_state.cs_input
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
            wlsi::Request::Offset { x, y } => self.s_state.cs_attached_xy = Some((x, y)),
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
        x: i32,
        y: i32,
    ) {
        self.s_state.cs_buffer = buf;
        // stash x/y for the cursor position change
        self.s_state.cs_attached_xy = Some((x, y));
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
    fn commit(&mut self, atmos: &mut Atmosphere) {
        // Check if we are a synchronized subsurface. if this is true, then we need
        // to move our CommitState into the parent's state as a pending child and then
        // exit.
        if let Some(Role::subsurface(ss)) = &self.s_role {
            let mut subsurf = ss.lock().unwrap();

            if subsurf.is_synchronized(atmos) {
                log::debug!("Adding sync subsurf {:?} to pending commit", self.s_id);
                let state = self.s_state.clone_refresh();
                subsurf
                    .ss_parent
                    .lock()
                    .unwrap()
                    .s_state
                    .cs_children
                    .push(state);

                return;
            }
        }

        self.s_state.commit(atmos);

        // Commit any role state before we update window bits
        let surf_size = *atmos.a_surface_size.get(&self.s_id).unwrap();
        match &self.s_role {
            Some(Role::xdg_shell_toplevel(_, xs)) => xs.lock().unwrap().commit(&self, atmos),
            Some(Role::xdg_shell_popup(xs)) => xs.lock().unwrap().commit(&self, atmos),
            Some(Role::wl_shell_toplevel) => atmos.a_window_size.set(&self.s_id, surf_size),
            Some(Role::subsurface(_)) => {}
            Some(Role::cursor) => {}
            None => {}
        }
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
        self.s_state.cs_frame_callbacks.push(callback);
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
