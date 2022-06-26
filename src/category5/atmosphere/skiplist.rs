// Support code for handling window heirarchies
//
// Austin Shafer - 2020

use super::*;
use crate::category5::input::Input;
use crate::category5::vkcomp::wm::task::Task;
use utils::{log, ClientId, WindowId};

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
    pub fn skiplist_remove_window(&mut self, id: WindowId) {
        let next = self.get_skiplist_next(id);
        let prev = self.get_skiplist_prev(id);

        // TODO: recalculate skip
        if let Some(p) = prev {
            self.set_skiplist_next(p, next);
        }
        if let Some(n) = next {
            self.set_skiplist_prev(n, prev);
        }

        // If this id is the first subsurface, then we need
        // to remove it from the parent
        if let Some(parent) = self.get_parent_window(id) {
            if let Some(top_child) = self.get_top_child(parent) {
                if top_child == id {
                    // Select the next subsurface
                    self.set_top_child(parent, next);
                }
            }
        }
    }

    /// Remove id from the `win_focus` visibility skiplist
    pub fn skiplist_remove_win_focus(&mut self, id: WindowId) {
        if let Some(focus) = self.get_win_focus() {
            // verify that we are actually removing the focused win
            if id == focus {
                // get the next node in the skiplist
                let next = self.get_skiplist_next(id);
                // clear its prev pointer (since it should be id)
                if let Some(n) = next {
                    self.set_skiplist_prev(n, None);
                }
                // actually update the focus
                self.set_win_focus(next);
                // clear id's pointers
                self.set_skiplist_next(id, None);
                self.set_skiplist_prev(id, None);
            }
        }
    }

    /// Remove id from the `surf_focus` property.
    /// This assumes that the `win_focus` has been set properly. i.e.
    /// call `skiplist_remove_win_focus` first.
    pub fn skiplist_remove_surf_focus(&mut self, id: WindowId) {
        if let Some(focus) = self.get_surf_focus() {
            // verify that we are actually removing the focused surf
            if id == focus {
                let root = self.get_root_window(id);
                let next_root = self.get_win_focus();
                if root.is_some() {
                    let next = match next_root {
                        Some(nr) => self.get_top_child(nr),
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
    pub fn skiplist_place_above(&mut self, id: WindowId, target: WindowId) {
        // remove id from its skiplist just in case
        self.skiplist_remove_window(id);

        // TODO: recalculate skip
        let prev = self.get_skiplist_prev(target);
        if let Some(p) = prev {
            self.set_skiplist_next(p, Some(id));
        }
        self.set_skiplist_prev(target, Some(id));

        // Now point id to the target and its neighbor
        self.set_skiplist_prev(id, prev);
        self.set_skiplist_next(id, Some(target));
        // generate add above event
    }

    /// Add a window below another
    ///
    /// This is used for the subsurface ordering requests
    pub fn skiplist_place_below(&mut self, id: WindowId, target: WindowId) {
        // remove id from its skiplist just in case
        self.skiplist_remove_window(id);

        // TODO: recalculate skip
        let next = self.get_skiplist_next(target);
        if let Some(n) = next {
            self.set_skiplist_prev(n, Some(id));
        }
        self.set_skiplist_next(target, Some(id));

        // Now point id to the target and its neighbor
        self.set_skiplist_prev(id, Some(target));
        self.set_skiplist_next(id, next);
        // generate add below event
    }

    /// Get the client in focus.
    /// This is better for subsystems like input which need to
    /// find the seat of the client currently in use.
    pub fn get_client_in_focus(&self) -> Option<ClientId> {
        // get the surface in focus
        if let Some(win) = self.get_win_focus() {
            // now get the client for that surface
            return Some(self.get_owner(win));
        }
        return None;
    }

    /// Get the root window in focus.
    ///
    /// A root window is the base of a subsurface tree. i.e. the toplevel surf
    /// that all subsurfaces are attached to.
    pub fn get_root_win_in_focus(&self) -> Option<WindowId> {
        if let Some(win) = self.get_win_focus() {
            return match self.get_root_window(win) {
                Some(root) => Some(root),
                // If win doesn't have a root window, it is the root window
                None => Some(win),
            };
        }
        return None;
    }

    /// Set the window currently in focus
    pub fn focus_on(&mut self, win: Option<WindowId>) {
        log::debug!("focusing on window {:?}", win);

        if let Some(id) = win {
            let root = self.get_root_window(id);
            // check if a new app was selected
            let prev_win_focus = self.get_win_focus();
            if let Some(prev) = prev_win_focus {
                let mut update_app = false;
                if let Some(r) = root {
                    if r != prev {
                        update_app = true;
                    } else {
                        // If this window is already selected, just bail
                        return;
                    }
                } else if prev != id {
                    // If the root window was None, then win *is* a root
                    // window, and we still need to check it
                    update_app = true;
                }

                // if so, update window focus
                if update_app {
                    // point the previous focus at the new focus
                    self.set_skiplist_prev(prev, win);

                    // Send leave event(s) to the old focus
                    Input::keyboard_leave(self, prev);
                }
            }

            // If no root window is attached, then win is a root window and
            // we need to update the win focus
            if root.is_none() {
                self.skiplist_remove_window(id);
                self.set_skiplist_next(id, prev_win_focus);
                self.set_skiplist_prev(id, None);
                self.set_win_focus(win);
                // Tell vkcomp to reorder its surface list. This is tricky,
                // since we want to keep a separation between the two subsystems,
                // and we want to avoid having to scan the skiplist to calculate
                // which ids need updating. It gets gross, particularly when
                // subsurfaces are involved. So we feed vkcomp an event stream
                // telling it to update thundr's surfacelist like we did the
                // skiplist.
                if let Some(id) = win {
                    self.add_wm_task(Task::move_to_front(id));
                }
            }
            // When focus changes between subsurfaces, we don't change the order. Only
            // wl_subsurface changes the order
            // set win to the surf focus
            self.set_surf_focus(win);
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
    pub fn add_new_top_subsurf(&mut self, parent: WindowId, win: WindowId) {
        log::info!("Adding subsurface {:?} to {:?}", win, parent);
        // Add the immediate parent
        self.set_parent_window(win, Some(parent));

        // add the root window for this subsurface tree
        // If the parent's root is None, then the parent is the root
        match self.get_root_window(parent) {
            Some(root) => self.set_root_window(win, Some(root)),
            None => self.set_root_window(win, Some(parent)),
        };

        // Add ourselves to the top of the skiplist
        let old_top = self.get_top_child(parent);
        if let Some(top) = old_top {
            self.skiplist_place_above(win, top);
        }

        self.set_top_child(parent, Some(win));
        // generate NewSubsurface event
        self.add_wm_task(Task::new_subsurface {
            id: win,
            parent: parent,
        });
    }

    /// Checks if the point (x, y) overlaps with the window surface.
    ///
    /// This does not accound for any regions, just the surface size.
    pub fn surface_is_at_point(&self, win: WindowId, barsize: f32, x: f32, y: f32) -> bool {
        log::info!("surface_is_at_point(win={:?}, x={}, y={})", win, x, y);
        let (wx, wy) = self.get_surface_pos(win);
        let (ww, wh) = self.get_surface_size(win);
        log::info!("surface {:?} pos x={}, y={}", win, wx, wy);
        log::info!("surface {:?} size x={}, y={}", win, ww, wh);

        // Ugly:
        // For the barsize to be included in our calculations,
        // we need to be sure that win is a root window, since only
        // root windows will have server-side decorations.
        let bs = match self.get_root_window(win) {
            Some(_) => 0.0, // Don't use a barsize offset
            None => barsize,
        };

        // If this window contains (x, y) then return it
        x > wx && y > (wy - bs) && x < (wx + ww) && y < (wy + wh)
    }

    /// Find the window id of the top window whose input region contains (x, y).
    ///
    /// In the case of delivering input enter/leave events, we don't just check
    /// which window contains the point, we need to check if windows with an
    /// input region contain the point.
    pub fn find_window_with_input_at_point(&self, x: f32, y: f32) -> Option<WindowId> {
        log::info!("find_window_with_input_at_point {},{}", x, y);
        let barsize = self.get_barsize();
        let mut ret = None;

        self.map_inorder_on_surfs(|win| {
            log::info!("checking window {:?}", win);
            // We need to get t
            if let Some(surf_cell) = self.get_surface_from_id(win) {
                let surf = surf_cell.borrow();
                let (wx, wy) = self.get_surface_pos(win);

                if let Some(input_region) = surf.s_input.as_ref() {
                    log::info!(
                        "Checking input region {:?} against {:?}",
                        input_region,
                        ((x - wx) as i32, (y - wy) as i32)
                    );
                    // Get the adjusted position of the input region
                    // based on the surface's position.
                    // The wl_region::Region doesn't track this, so
                    // our (ugly) hack for this is to reduce (x, y)
                    // by the position of the window, instead of scaling
                    // every Rect in the Region up by that amount
                    if input_region
                        .borrow()
                        .intersects((x - wx) as i32, (y - wy) as i32)
                    {
                        ret = Some(win);
                        return false;
                    }
                } else {
                    // TODO: VERIFY
                    // If the window does not have an attached input region,
                    // then we need to check against the entire surface area.
                    if self.surface_is_at_point(win, barsize, x, y) {
                        ret = Some(win);
                        return false;
                    }
                }
            }
            return true;
        });

        log::info!("Found window {:?}", ret);
        return ret;
    }

    /// Find if there is a toplevel window under (x,y)
    ///
    /// This is used first to find if the cursor intersects
    /// with a window. If it does, point_is_on_titlebar is
    /// used to check for a grab or relay input event.
    pub fn find_window_at_point(&self, x: f32, y: f32) -> Option<WindowId> {
        let barsize = self.get_barsize();

        let mut ret = None;
        self.map_inorder_on_surfs(|win| {
            if self.surface_is_at_point(win, barsize, x, y) {
                ret = Some(win);
                return false;
            }
            // returning true tells the map function to keep executing
            return true;
        });

        return ret;
    }

    /// Is the current point over the titlebar of the window
    ///
    /// Id should have first been found with find_window_at_point
    pub fn point_is_on_titlebar(&self, id: WindowId, x: f32, y: f32) -> bool {
        let barsize = self.get_barsize();
        let (wx, wy) = self.get_surface_pos(id);
        let (ww, _wh) = self.get_surface_size(id);

        // If this window contains (x, y) then return it
        if x > wx && y > (wy - barsize) && x < (wx + ww) && y < wy {
            return true;
        }
        return false;
    }

    /// calculates if a position is over the part of a window that
    /// procs a resize
    pub fn point_is_on_window_edge(&self, id: WindowId, x: f32, y: f32) -> ResizeEdge {
        let barsize = self.get_barsize();
        // TODO: how should this be done with xdg-decoration?
        let (wx, wy) = self.get_surface_pos(id);
        let (ww, wh) = self.get_surface_size(id);
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
    fn map_on_surf_tree_recurse<F>(&self, inorder: bool, win: WindowId, func: &mut F) -> bool
    where
        F: FnMut(WindowId) -> bool,
    {
        // First recursively check all subsurfaces
        for sub in self.visible_subsurfaces(win) {
            // If we are going out of order, the only difference is we call
            // func beforehand
            if !inorder {
                if !func(sub) {
                    return false;
                }
            }
            if !self.map_on_surf_tree_recurse(inorder, sub, func) {
                return false;
            }
            if inorder {
                if !func(sub) {
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
        F: FnMut(WindowId) -> bool,
    {
        for win in self.visible_windows() {
            if !self.map_on_surf_tree_recurse(inorder, win, &mut func) {
                return;
            }
            if !func(win) {
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
        F: FnMut(WindowId) -> bool,
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
        F: FnMut(WindowId) -> bool,
    {
        self.map_on_surfs(false, func)
    }

    pub fn print_surface_tree(&self) {
        log::debug!("Dumping surface tree (front to back):");
        self.map_inorder_on_surfs(|_win| {
            log::debug!(
                " - {:?}   windims at {:?} size {:?} surfdims at {:?} size {:?}",
                _win,
                self.get_window_pos(_win),
                self.get_surface_pos(_win),
                self.get_window_size(_win),
                self.get_surface_size(_win),
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
    pub fn visible_subsurfaces(&'a self, id: WindowId) -> VisibleWindowIterator<'a> {
        VisibleWindowIterator {
            vwi_atmos: &self,
            vwi_cur: self.get_top_child(id),
        }
    }
}

// Iterator for visible windows in a desktop
pub struct VisibleWindowIterator<'a> {
    vwi_atmos: &'a Atmosphere,
    // the current window we are on
    vwi_cur: Option<WindowId>,
}

// Non-consuming iterator over an Atmosphere
//
// This will only show the visible windows
impl<'a> IntoIterator for &'a Atmosphere {
    type Item = WindowId;
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
    // Our item type is a WindowId
    type Item = WindowId;

    fn next(&mut self) -> Option<WindowId> {
        let ret = self.vwi_cur.take();
        // TODO: actually skip
        if let Some(id) = ret {
            self.vwi_cur = self.vwi_atmos.get_skiplist_next(id);
        }

        return ret;
    }
}
