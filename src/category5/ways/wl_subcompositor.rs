// Implementation of wl_subsurface and wl_subcompositor
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::protocol::wl_subcompositor;
use ws::protocol::wl_subsurface;
use ws::Resource;

use super::role::Role;
use super::surface::Surface;
use crate::category5::atmosphere::{Atmosphere, SurfaceId};
use crate::category5::Climate;

use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

#[allow(unused_variables)]
impl ws::GlobalDispatch<wl_subcompositor::WlSubcompositor, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<wl_subcompositor::WlSubcompositor>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wl_subcompositor::WlSubcompositor, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_subcompositor::WlSubcompositor,
        request: wl_subcompositor::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        match request {
            wl_subcompositor::Request::GetSubsurface {
                id,
                surface,
                parent: par,
            } => {
                // get category5's surface from the userdata
                let surf = surface.data::<Arc<Mutex<Surface>>>().unwrap().clone();
                let parent = par.data::<Arc<Mutex<Surface>>>().unwrap().clone();

                // TODO: throw error if surface has another role

                let ss = Arc::new(Mutex::new(SubSurface::new(
                    state.c_atmos.lock().unwrap().deref_mut(),
                    surf.clone(),
                    parent,
                )));
                // Mark this surface with the subsurface roll
                surf.lock().unwrap().s_role = Some(Role::subsurface(ss.clone()));
                // Add our subsurface as the userdata
                data_init.init(id, ss);
            }
            _ => (),
        };
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &(),
    ) {
    }
}

#[allow(unused_variables)]
impl ws::Dispatch<wl_subsurface::WlSubsurface, Arc<Mutex<SubSurface>>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_subsurface::WlSubsurface,
        request: wl_subsurface::Request,
        data: &Arc<Mutex<SubSurface>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data.lock()
            .unwrap()
            .handle_request(state.c_atmos.lock().unwrap().deref_mut(), request);
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &Arc<Mutex<SubSurface>>,
    ) {
    }
}

/// This tracks the double buffered state for a subsurface
///
/// Subsurfaces are really no different than actual surfaces,
/// except this interface is their role and they have a slightly
/// different rendering logic path.
#[allow(dead_code)]
pub struct SubSurface {
    ss_surf: Arc<Mutex<Surface>>,
    ss_parent: Arc<Mutex<Surface>>,
    /// attached new position to be applied on commit
    ss_position: Option<(f32, f32)>,
    /// these requests reorder our skiplist
    ss_place_above: Option<SurfaceId>,
    ss_place_below: Option<SurfaceId>,
    /// This will be set to true if we are a synchronized subsurface
    /// and we are waiting for the parent to commit to actually
    /// perform our own commit.
    ///
    /// This will be set when a sync subsurface is committed directly,
    /// aka not from the parent's commit.
    ss_sync_committed: bool,
}

impl SubSurface {
    fn new(
        atmos: &mut Atmosphere,
        surf_lock: Arc<Mutex<Surface>>,
        parent: Arc<Mutex<Surface>>,
    ) -> Self {
        let surf = surf_lock.lock().unwrap();
        // We need to mark this surface as the new top child
        // of the parent
        atmos.add_new_top_subsurf(&parent.lock().unwrap().s_id, &surf.s_id);

        // The synchronized state defaults to true
        atmos.a_subsurface_sync.set(&surf.s_id, true);

        Self {
            ss_surf: surf_lock.clone(),
            ss_parent: parent,
            ss_position: None,
            ss_place_above: None,
            ss_place_below: None,
            ss_sync_committed: false,
        }
    }

    fn handle_request(&mut self, atmos: &mut Atmosphere, req: wl_subsurface::Request) {
        match req {
            wl_subsurface::Request::SetPosition { x, y } => {
                self.ss_position = Some((x as f32, y as f32))
            }
            wl_subsurface::Request::PlaceAbove { sibling } => {
                self.ss_place_above = Some(
                    sibling
                        .data::<Arc<Mutex<Surface>>>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .s_id
                        .clone(),
                )
            }
            wl_subsurface::Request::PlaceBelow { sibling } => {
                self.ss_place_below = Some(
                    sibling
                        .data::<Arc<Mutex<Surface>>>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .s_id
                        .clone(),
                )
            }
            wl_subsurface::Request::SetSync => atmos
                .a_subsurface_sync
                .set(&self.ss_surf.lock().unwrap().s_id, true),
            wl_subsurface::Request::SetDesync => atmos
                .a_subsurface_sync
                .set(&self.ss_surf.lock().unwrap().s_id, false),
            _ => (),
        };
    }

    /// There are two scenarios where we are okay to commit a subsurface:
    /// * The subsurface (or any of its parents) is synchronized
    /// * The subsurface is desynchronized, and the subsurface itself is
    /// being committed.
    ///
    /// This function checks for both scenarios and returns true if
    /// a commit should be performed.
    pub fn should_commit(
        &mut self,
        surf: &Surface,
        atmos: &mut Atmosphere,
        parent_commit_in_progress: bool,
    ) -> bool {
        let is_sync = match atmos.a_subsurface_sync.get_clone(&surf.s_id) {
            Some(true) => true,
            Some(false) => {
                // We got to check all the parent surfaces, if any of them
                // are synchronized then we are too
                let mut win = surf.s_id.clone();
                let mut sync = false;

                while let Some(parent) = atmos.a_parent_window.get_clone(&win) {
                    if let Some(parent_sync) = atmos.a_subsurface_sync.get_clone(&parent) {
                        if parent_sync {
                            sync = true;
                            break;
                        }
                    }
                    win = parent.clone();
                }
                sync
            }
            None => panic!("Invalid subsurface state"),
        };

        if (is_sync && self.ss_sync_committed) || (!is_sync && !parent_commit_in_progress) {
            return true;
        } else if is_sync && !parent_commit_in_progress {
            // In this case this subsurface was committed by the app, and we should mark
            // it as ready to be committed on the next parent commit.
            self.ss_sync_committed = true;
        }

        return false;
    }

    /// Apply all of our state
    ///
    /// This is called in an extremely recursive fashion from Surface::commit, so
    /// the surface, atmosphere, and in progress flags are all arguments.
    pub fn commit(&mut self, surf: &Surface, atmos: &mut Atmosphere) {
        let id = &surf.s_id;

        // set_position request
        if let Some((x, y)) = self.ss_position {
            atmos.a_surface_pos.set(id, (x, y));
        }
        self.ss_position = None;

        // place_above
        if let Some(target) = self.ss_place_above.as_ref() {
            atmos.skiplist_place_above(id, target);
        }
        self.ss_place_above = None;
        // place_below
        if let Some(target) = self.ss_place_below.as_ref() {
            atmos.skiplist_place_below(id, target);
        }
        self.ss_place_below = None;
    }
}
