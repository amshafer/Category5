// Implementation of wl_subsurface and wl_subcompositor
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::protocol::wl_subcompositor as wlsc;
use ws::protocol::wl_subsurface as wlss;
use ws::Main;

use super::role::Role;
use super::surface::Surface;
use crate::category5::atmosphere::Atmosphere;
use utils::WindowId;

use std::cell::RefCell;
use std::rc::Rc;

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
                .get::<Rc<RefCell<Surface>>>()
                .unwrap()
                .clone();
            let parent = par
                .as_ref()
                .user_data()
                .get::<Rc<RefCell<Surface>>>()
                .unwrap()
                .clone();

            // TODO: throw error if surface has another role

            let ss = Rc::new(RefCell::new(SubSurface::new(
                id.clone(),
                surf.clone(),
                parent,
            )));
            surf.borrow_mut().s_role = Some(Role::subsurface(ss.clone()));
            id.quick_assign(move |_, r, _| {
                let mut ssurf = ss.borrow_mut();
                ssurf.handle_request(r);
            });
        }
        _ => (),
    };
}

// This tracks the double buffered state for a subsurface
//
// Subsurfaces are really no different than actual surfaces,
// except this interface is their role and they have a slightly
// different rendering logic path.
#[allow(dead_code)]
pub struct SubSurface {
    ss_atmos: Rc<RefCell<Atmosphere>>,
    ss_proxy: Main<wlss::WlSubsurface>,
    ss_surf: Rc<RefCell<Surface>>,
    ss_parent: Rc<RefCell<Surface>>,
    // attached new position to be applied on commit
    ss_position: Option<(f32, f32)>,
    // these requests reorder our skiplist
    ss_place_above: Option<WindowId>,
    ss_place_below: Option<WindowId>,
    // Is this surface committed with the parent?
    ss_sync: bool,
}

impl SubSurface {
    fn new(
        sub: Main<wlss::WlSubsurface>,
        surf: Rc<RefCell<Surface>>,
        parent: Rc<RefCell<Surface>>,
    ) -> Self {
        let atmos = surf.borrow_mut().s_atmos.clone();

        // We need to mark this surface as the new top child
        // of the parent
        atmos
            .borrow_mut()
            .add_new_top_subsurf(parent.borrow().s_id, surf.borrow().s_id);

        Self {
            ss_atmos: atmos,
            ss_proxy: sub,
            ss_surf: surf,
            ss_parent: parent,
            ss_position: None,
            ss_place_above: None,
            ss_place_below: None,
            ss_sync: true,
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
                        .get::<Rc<RefCell<Surface>>>()
                        .unwrap()
                        .borrow()
                        .s_id,
                )
            }
            wlss::Request::PlaceBelow { sibling } => {
                self.ss_place_below = Some(
                    sibling
                        .as_ref()
                        .user_data()
                        .get::<Rc<RefCell<Surface>>>()
                        .unwrap()
                        .borrow()
                        .s_id,
                )
            }
            wlss::Request::SetSync => self.ss_sync = true,
            wlss::Request::SetDesync => self.ss_sync = false,
            _ => (),
        };
    }

    /// Apply all of our state
    ///
    /// This is called in an extremely recursive fashion from Surface::commit, so
    /// the surface, atmosphere, and in progress flags are all arguments.
    pub fn commit(&mut self, surf: &Surface, atmos: &mut Atmosphere, commit_in_progress: bool) {
        // If commit is called but we are in sync mode and the parent
        // is not committing, then do nothing
        if self.ss_sync && commit_in_progress {
            return;
        }

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
