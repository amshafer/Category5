// Implementation of wl_subsurface and wl_subcompositor
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::protocol::wl_subcompositor as wlsc;
use ws::protocol::wl_subsurface as wlss;

use super::role::Role;
use super::surface::Surface;
use crate::category5::atmosphere::Atmosphere;
use utils::WindowId;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub fn wl_subcompositor_handle_request(req: wlsc::Request, _: Main<wlsc::WlSubcompositor>) {
    match req {
        wlsc::Request::GetSubsurface {
            id,
            surface,
            parent: par,
        } => {
            // get category5's surface from the userdata
            let surf = surface
                .as_ref()
                .user_data()
                .get::<Arc<Mutex<Surface>>>()
                .unwrap()
                .clone();
            let parent = par
                .as_ref()
                .user_data()
                .get::<Arc<Mutex<Surface>>>()
                .unwrap()
                .clone();

            // TODO: throw error if surface has another role

            let ss = Rc::new(RefCell::new(SubSurface::new(
                id.clone(),
                surf.clone(),
                parent,
            )));
            surf.lock().unwrap().s_role = Some(Role::subsurface(ss.clone()));
            id.quick_assign(move |_, r, _| {
                let mut ssurf = ss.lock().unwrap();
                ssurf.handle_request(r);
            });
        }
        _ => (),
    };
}

/// This tracks the double buffered state for a subsurface
///
/// Subsurfaces are really no different than actual surfaces,
/// except this interface is their role and they have a slightly
/// different rendering logic path.
#[allow(dead_code)]
pub struct SubSurface {
    ss_atmos: Arc<Mutex<Atmosphere>>,
    ss_proxy: wlss::WlSubsurface,
    ss_surf: Arc<Mutex<Surface>>,
    ss_parent: Arc<Mutex<Surface>>,
    /// attached new position to be applied on commit
    ss_position: Option<(f32, f32)>,
    /// these requests reorder our skiplist
    ss_place_above: Option<WindowId>,
    ss_place_below: Option<WindowId>,
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
        sub: wlss::WlSubsurface,
        surf: Arc<Mutex<Surface>>,
        parent: Arc<Mutex<Surface>>,
    ) -> Self {
        let atmos_mtx = surf.lock().unwrap().s_atmos.clone();
        {
            let mut atmos = atmos_mtx.lock().unwrap();
            // We need to mark this surface as the new top child
            // of the parent
            atmos.add_new_top_subsurf(parent.lock().unwrap().s_id, surf.lock().unwrap().s_id);

            // The synchronized state defaults to true
            atmos.set_subsurface_sync(surf.lock().unwrap().s_id, Some(true));
        }

        Self {
            ss_atmos: atmos_mtx,
            ss_proxy: sub,
            ss_surf: surf,
            ss_parent: parent,
            ss_position: None,
            ss_place_above: None,
            ss_place_below: None,
            ss_sync_committed: false,
        }
    }

    fn handle_request(&mut self, req: wlss::Request) {
        match req {
            wlss::Request::SetPosition { x, y } => self.ss_position = Some((x as f32, y as f32)),
            wlss::Request::PlaceAbove { sibling } => {
                self.ss_place_above = Some(
                    sibling
                        .as_ref()
                        .user_data()
                        .get::<Arc<Mutex<Surface>>>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .s_id,
                )
            }
            wlss::Request::PlaceBelow { sibling } => {
                self.ss_place_below = Some(
                    sibling
                        .as_ref()
                        .user_data()
                        .get::<Arc<Mutex<Surface>>>()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .s_id,
                )
            }
            wlss::Request::SetSync => self
                .ss_atmos
                .lock()
                .unwrap()
                .set_subsurface_sync(self.ss_surf.lock().unwrap().s_id, Some(true)),
            wlss::Request::SetDesync => self
                .ss_atmos
                .lock()
                .unwrap()
                .set_subsurface_sync(self.ss_surf.lock().unwrap().s_id, Some(false)),
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
        let is_sync = match atmos.get_subsurface_sync(surf.s_id) {
            Some(true) => true,
            Some(false) => {
                // We got to check all the parent surfaces, if any of them
                // are synchronized then we are too
                let mut win = surf.s_id;
                let mut sync = false;

                while let Some(parent) = atmos.get_parent_window(win) {
                    if let Some(parent_sync) = atmos.get_subsurface_sync(parent) {
                        if parent_sync {
                            sync = true;
                            break;
                        }
                    }
                    win = parent;
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
        let id = surf.s_id;

        // set_position request
        if let Some((x, y)) = self.ss_position {
            atmos.set_surface_pos(id, x, y);
        }
        self.ss_position = None;

        // place_above
        if let Some(target) = self.ss_place_above {
            atmos.skiplist_place_above(id, target);
        }
        self.ss_place_above = None;
        // place_below
        if let Some(target) = self.ss_place_below {
            atmos.skiplist_place_below(id, target);
        }
        self.ss_place_below = None;
    }
}
