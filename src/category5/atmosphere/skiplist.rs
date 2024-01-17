// Support code for handling window heirarchies
//
// Austin Shafer - 2020
extern crate wayland_protocols;
use wayland_protocols::xdg::shell::server::xdg_toplevel::ResizeEdge;

use super::*;
use crate::category5::input::Input;
use crate::category5::vkcomp::wm::task::Task;
use utils::log;

// A skiplist is an entry in a linked list designed to be
// added in the atmosphere's property system
//
// The idea is that each window has one of these
// which points to the next and previous windows in
// the global ordering for that desktop. These properties
// will be consistently published by the atmosphere just
// like the rest.

impl Atmosphere {
    /// Removes a window from the heirarchy.
    ///
    /// Use this to pull a window out, and then insert it in focus
    pub fn skiplist_remove_window(&mut self, id: &SurfaceId) {
        let next = self.a_skiplist_next.get_clone(id);
        let prev = self.a_skiplist_prev.get_clone(id);

        // TODO: recalculate skip
        if let Some(p) = prev.as_ref() {
            self.a_skiplist_next.set_opt(p, next.clone());
        }
        if let Some(n) = next.as_ref() {
            self.a_skiplist_prev.set_opt(n, prev.clone());
        }

        // If this id is the first subsurface, then we need
        // to remove it from the parent
        if let Some(parent) = self.a_parent_window.get_clone(id) {
            if let Some(top_child) = self.a_top_child.get_clone(&parent) {
                if &top_child == id {
                    // Select the next subsurface
                    self.a_top_child.set_opt(&parent, next);
                }
            }
        }
    }

    /// Remove id from the `win_focus` visibility skiplist
    pub fn skiplist_remove_win_focus(&mut self, id: &SurfaceId) {
        if let Some(focus) = self.get_win_focus() {
            // verify that we are actually removing the focused win
            if id == &focus {
                // get the next node in the skiplist
                let next = self.a_skiplist_next.get_clone(id);
                // clear its prev pointer (since it should be id)
                if let Some(n) = next.as_ref() {
                    self.a_skiplist_prev.take(n);
                }
                // actually update the focus
                self.set_win_focus(next);
                // clear id's pointers
                self.a_skiplist_next.take(id);
                self.a_skiplist_prev.take(id);
            }
        }
    }

    /// Remove id from the `surf_focus` property.
    /// This assumes that the `win_focus` has been set properly. i.e.
    /// call `skiplist_remove_win_focus` first.
    pub fn skiplist_remove_surf_focus(&mut self, id: &SurfaceId) {
        if let Some(focus) = self.get_surf_focus() {
            // verify that we are actually removing the focused surf
            if id == &focus {
                let next_root = self.get_win_focus();
                if self.a_root_window.get(id).is_some() {
                    let next = match next_root {
                        Some(nr) => self.a_top_child.get_clone(&nr),
                        None => None,
                    };
                    self.set_surf_focus(next);
                } else {
                    // If the root is none, then we are removing a root
                    // window from focus. This means we should set the focus
                    // to whatever the win focus is, since it has been updated
                    // with the next root window
                    self.set_surf_focus(next_root);
                }
            }
        }
    }

    /// Add a window above another
    ///
    /// This is used for the subsurface ordering requests
    pub fn skiplist_place_above(&mut self, id: &SurfaceId, target: &SurfaceId) {
        // remove id from its skiplist just in case
        self.skiplist_remove_window(id);

        // TODO: recalculate skip
        let prev = self.a_skiplist_prev.get_clone(target);
        if let Some(p) = prev.as_ref() {
            self.a_skiplist_next.set(p, id.clone());
        }
        self.a_skiplist_prev.set(target, id.clone());

        // Now point id to the target and its neighbor
        self.a_skiplist_prev.set_opt(id, prev);
        self.a_skiplist_next.set(id, target.clone());
        // generate add above event
    }

    /// Add a window below another
    ///
    /// This is used for the subsurface ordering requests
    pub fn skiplist_place_below(&mut self, id: &SurfaceId, target: &SurfaceId) {
        // remove id from its skiplist just in case
        self.skiplist_remove_window(id);

        // TODO: recalculate skip
        let next = self.a_skiplist_next.get_clone(&target);
        if let Some(n) = next.as_ref() {
            self.a_skiplist_prev.set(n, id.clone());
        }
        self.a_skiplist_next.set(target, id.clone());

        // Now point id to the target and its neighbor
        self.a_skiplist_next.set_opt(id, next);
        self.a_skiplist_prev.set(id, target.clone());
        // generate add below event
    }

    /// Get the client in focus.
    /// This is better for subsystems like input which need to
    /// find the seat of the client currently in use.
    pub fn get_client_in_focus(&self) -> Option<ClientId> {
        // get the surface in focus
        if let Some(win) = self.get_win_focus() {
            // now get the client for that surface
            return self.a_owner.get_clone(&win);
        }
        return None;
    }

    /// Get the root window in focus.
    ///
    /// A root window is the base of a subsurface tree. i.e. the toplevel surf
    /// that all subsurfaces are attached to.
    pub fn get_root_win_in_focus(&self) -> Option<SurfaceId> {
        if let Some(win) = self.get_win_focus() {
            return match self.a_root_window.get_clone(&win) {
                Some(root) => Some(root),
                // If win doesn't have a root window, it is the root window
                None => Some(win),
            };
        }
        return None;
    }

    /// Set the window currently in focus
    pub fn focus_on(&mut self, win: Option<SurfaceId>) {
        log::debug!("focusing on window {:?}", win);

        if let Some(id) = win.as_ref() {
            // check if a new app was selected
            let root = self.a_root_window.get_clone(id);
            let prev_win_focus = self.get_win_focus();
            if let Some(prev) = prev_win_focus.as_ref() {
                // Check if we need to change focus. We either compare with this
                // window or the root app window, if we have one.
                let cur = match root.as_ref() {
                    Some(r) => r,
                    None => id,
                };

                if cur != prev {
                    // point the previous focus at the new focus
                    self.a_skiplist_prev.set(&prev, id.clone());

                    // Send leave event(s) to the old focus
                    Input::keyboard_leave(self, &prev);
                } else {
                    // Otherwise this window is already in focus, so bail
                    return;
                }
            }

            // If no root window is attached, then win is a root window and
            // we need to update the win focus
            if root.is_none() {
                self.skiplist_remove_window(id);
                self.a_skiplist_next.set_opt(id, prev_win_focus);
                self.a_skiplist_prev.set_opt(id, None);
                self.set_win_focus(Some(id.clone()));
                // Tell vkcomp to reorder its surface list. This is tricky,
                // since we want to keep a separation between the two subsystems,
                // and we want to avoid having to scan the skiplist to calculate
                // which ids need updating. It gets gross, particularly when
                // subsurfaces are involved. So we feed vkcomp an event stream
                // telling it to update thundr's surfacelist like we did the
                // skiplist.
                self.add_wm_task(Task::move_to_front(id.clone()));
            }
            // When focus changes between subsurfaces, we don't change the order. Only
            // wl_subsurface changes the order
            // set win to the surf focus
            self.set_surf_focus(Some(id.clone()));
            // Send enter event(s) to the new focus
            // spec says this MUST be done after the leave events are sent
            Input::keyboard_enter(self, id);
        } else {
            // Otherwise we have unselected any surfaces, so clear both focus types
            self.set_win_focus(None);
            self.set_surf_focus(None);
        }
        self.print_surface_tree();

        // TODO: recalculate skip
    }

    /// Adds the surface `win` as the top subsurface of `parent`.
    pub fn add_new_top_subsurf(&mut self, parent: &SurfaceId, win: &SurfaceId) {
        log::info!(
            "Adding subsurface {:?} to {:?}",
            win.get_raw_id(),
            parent.get_raw_id()
        );
        // Add the immediate parent
        self.a_parent_window.set(win, parent.clone());

        // set the root window for this subsurface tree
        // If the parent's root is None, then the parent is the root
        match self.a_root_window.get_clone(parent) {
            Some(root) => self.a_root_window.set(win, root),
            None => self.a_root_window.set(win, parent.clone()),
        };

        // Add ourselves to the top of the skiplist
        if let Some(top) = self.a_top_child.get_clone(parent) {
            self.skiplist_place_above(win, &top);
        }

        self.a_top_child.set(parent, win.clone());
        // generate NewSubsurface event
        self.add_wm_task(Task::new_subsurface {
            id: win.clone(),
            parent: parent.clone(),
        });
    }

    /// Convert a global position in the screen to a position
    /// within the usable desktop region. This offsets the menubar
    /// at the top of the screen
    pub fn get_adjusted_desktop_coord(&self, x: f32, y: f32) -> (f32, f32) {
        (x, y - wm::DESKTOP_OFFSET as f32)
    }

    /// Find the window id of the top window whose input region contains (x, y).
    ///
    /// In the case of delivering input enter/leave events, we don't just check
    /// which window contains the point, we need to check if windows with an
    /// input region contain the point.
    pub fn find_window_with_input_at_point(&self, x: f32, y: f32) -> Option<SurfaceId> {
        log::debug!("find_window_with_input_at_point {},{}", x, y);
        let mut ret = None;
        // Adjust for offsetting into the desktop
        let adjusted = self.get_adjusted_desktop_coord(x, y);
        log::debug!("Adjusted pos {:?}", adjusted);

        self.map_ooo_on_surfs(|win, offset| {
            log::debug!("checking window {:?}", win.get_raw_id());
            let (wx, wy) = *self.a_surface_pos.get(&win).unwrap();
            log::debug!("Base offset {:?}", offset);
            log::debug!("Surface pos {:?}", (wx, wy));

            // reduce our x,y position by all offsets. This includes the
            // offset of this (sub)surface along with the base offset
            // of the parent surface
            let (x, y) = (adjusted.0 - wx - offset.0, adjusted.1 - wy - offset.1);
            log::debug!("Final pos {:?}", (x, y));

            if let Some(input_region) = self.a_input_region.get(&win) {
                log::debug!(
                    "Checking input region {:?} against {:?}",
                    *input_region,
                    (x as i32, y as i32)
                );

                // Get the adjusted position of the input region
                // based on the surface's position.
                // The wl_region::Region doesn't track this, so
                // our (ugly) hack for this is to reduce (x, y)
                // by the position of the window, instead of scaling
                // every Rect in the Region up by that amount
                if input_region.lock().unwrap().intersects(x as i32, y as i32) {
                    ret = Some(win);
                    return false;
                }
            } else {
                // If the window does not have an attached input region,
                // then we need to check against the entire surface area.
                let (ww, wh) = *self.a_surface_size.get(&win).unwrap();
                if x > 0.0 && y > 0.0 && x < ww && y < wh {
                    ret = Some(win);
                    return false;
                }
            }
            return true;
        });

        log::debug!("Found window {:?}", ret.as_ref().map(|id| id.get_raw_id()));
        return ret;
    }

    /// Is the current point over the titlebar of the window
    ///
    /// Id should have first been found with find_window_at_point
    pub fn point_is_on_titlebar(&self, id: &SurfaceId, x: f32, y: f32) -> bool {
        let barsize = self.get_barsize();
        let (wx, wy) = *self.a_surface_pos.get(id).unwrap();
        let (ww, _wh) = *self.a_surface_size.get(id).unwrap();

        // If this window contains (x, y) then return it
        if x > wx && y > (wy - barsize) && x < (wx + ww) && y < wy {
            return true;
        }
        return false;
    }

    /// calculates if a position is over the part of a window that
    /// procs a resize
    pub fn point_is_on_window_edge(&self, id: &SurfaceId, x: f32, y: f32) -> ResizeEdge {
        let barsize = self.get_barsize();
        // TODO: how should this be done with xdg-decoration?
        let (wx, wy) = *self.a_surface_pos.get(id).unwrap();
        let (ww, wh) = *self.a_surface_size.get(id).unwrap();
        let prox = 3.0; // TODO find a better val for this??

        // is (x,y) inside each dimension of the window
        let x_contained = x > wx && x < wx + ww;
        let y_contained = y > wy && y < wy + barsize + wh;

        // closures for helping us with overlap calculations
        // v is val to check, a is axis location
        let near_edge = |p, a| p > (a - prox) && p < (a + prox);
        // same thing but for corners
        // v is the point and c is the corner
        let near_corner = |vx, vy, cx, cy| near_edge(vx, cx) && near_edge(vy, cy);

        // first check if we are over a corner
        if near_corner(x, y, wx, wy) {
            ResizeEdge::TopLeft
        } else if near_corner(x, y, wx + ww, wy) {
            ResizeEdge::TopRight
        } else if near_corner(x, y, wx, wy + wh) {
            ResizeEdge::BottomLeft
        } else if near_corner(x, y, wx + ww, wy + wh) {
            ResizeEdge::BottomRight
        } else if near_edge(x, wx) && y_contained {
            ResizeEdge::Left
        } else if near_edge(x, wx + ww) && y_contained {
            ResizeEdge::Right
        } else if near_edge(y, wy) && x_contained {
            ResizeEdge::Top
        } else if near_edge(y, wy + wh) && x_contained {
            ResizeEdge::Bottom
        } else {
            ResizeEdge::None
        }
    }

    /// The recursive portion of `map_on_surfs`
    fn map_on_surf_tree_recurse<F>(
        &self,
        inorder: bool,
        win: SurfaceId,
        func: &mut F,
        mut offset: (f32, f32),
    ) -> bool
    where
        F: FnMut(SurfaceId, (f32, f32)) -> bool,
    {
        // recalculate our offset to start at this surface's offset.
        let pos = self.a_surface_pos.get(&win).unwrap();
        offset.0 += pos.0;
        offset.1 += pos.1;

        // First recursively check all subsurfaces
        for sub in self.visible_subsurfaces(&win) {
            // If we are going out of order, the only difference is we call
            // func beforehand
            if !inorder {
                if !func(sub.clone(), offset) {
                    return false;
                }
            }
            if !self.map_on_surf_tree_recurse(inorder, sub.clone(), func, offset) {
                return false;
            }
            if inorder {
                if !func(sub.clone(), offset) {
                    return false;
                }
            }
        }
        return true;
    }

    /// This is the generic map implementation, entrypoint to the recursive
    /// surface evaluation.
    fn map_on_surfs<F>(&self, inorder: bool, mut func: F)
    where
        F: FnMut(SurfaceId, (f32, f32)) -> bool,
    {
        for win in self.visible_windows() {
            if !self.map_on_surf_tree_recurse(inorder, win.clone(), &mut func, (0.0, 0.0)) {
                return;
            }
            if !func(win.clone(), (0.0, 0.0)) {
                return;
            }
        }
    }

    /// Helper for walking the surface tree recursively from front to
    /// back.
    /// `func` will be called on every window
    ///
    /// `func` returns a boolean specifying if the traversal should
    /// continue or exit.
    pub fn map_inorder_on_surfs<F>(&self, func: F)
    where
        F: FnMut(SurfaceId, (f32, f32)) -> bool,
    {
        self.map_on_surfs(true, func)
    }

    /// Map out of order on windows and subsurfaces.
    /// Helper for walking the surface tree recursively in the order
    /// of root windows, then subsurfaces in order.
    ///
    /// Basically this will call the root window, followed by its subsurfaces.
    /// This is extremely useful in calculations depending on the parent (i.e.)
    /// calculating the positions of subsurfaces.
    ///
    /// `func` will be called on every window
    ///
    /// `func` returns a boolean specifying if the traversal should
    /// continue or exit.
    pub fn map_ooo_on_surfs<F>(&self, func: F)
    where
        F: FnMut(SurfaceId, (f32, f32)) -> bool,
    {
        self.map_on_surfs(false, func)
    }

    pub fn print_surface_tree(&self) {
        log::debug!("Dumping surface tree (front to back):");
        self.map_inorder_on_surfs(|_win, _offset| {
            log::debug!(
                " - {:?}   windims at {:?} size {:?} surfdims at {:?} size {:?}",
                _win,
                *self.a_window_pos.get(&_win).unwrap(),
                *self.a_surface_pos.get(&_win).unwrap(),
                self.a_window_size.get(&_win).map(|ws| *ws),
                *self.a_surface_size.get(&_win).unwrap(),
            );
            // Return true to tell map_on_surfs to continue
            return true;
        });
    }
}

// (see PropertyMapIterator for lifetime comments
impl<'a> Atmosphere {
    /// return an iterator of valid ids.
    ///
    /// This will be all ids that are have been `activate`d
    pub fn visible_windows(&'a self) -> VisibleWindowIterator<'a> {
        self.into_iter()
    }

    /// return an iterator over the subsurfaces of id
    ///
    /// This will be all ids that are have been `activate`d
    pub fn visible_subsurfaces(&'a self, id: &SurfaceId) -> VisibleWindowIterator<'a> {
        VisibleWindowIterator {
            vwi_atmos: &self,
            vwi_cur: self.a_top_child.get_clone(id),
        }
    }
}

// Iterator for visible windows in a desktop
pub struct VisibleWindowIterator<'a> {
    vwi_atmos: &'a Atmosphere,
    // the current window we are on
    vwi_cur: Option<SurfaceId>,
}

// Non-consuming iterator over an Atmosphere
//
// This will only show the visible windows
impl<'a> IntoIterator for &'a Atmosphere {
    type Item = SurfaceId;
    type IntoIter = VisibleWindowIterator<'a>;

    // note that into_iter() is consuming self
    fn into_iter(self) -> Self::IntoIter {
        VisibleWindowIterator {
            vwi_atmos: &self,
            vwi_cur: self.get_win_focus(),
        }
    }
}

impl<'a> Iterator for VisibleWindowIterator<'a> {
    // Our item type is a SurfaceId
    type Item = SurfaceId;

    fn next(&mut self) -> Option<SurfaceId> {
        let ret = self.vwi_cur.take();
        // TODO: actually skip
        if let Some(id) = ret.as_ref() {
            self.vwi_cur = self.vwi_atmos.a_skiplist_next.get_clone(id);
        }

        return ret;
    }
}
