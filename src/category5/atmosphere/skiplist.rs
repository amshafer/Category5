// Support code for handling window heirarchies
//
// Austin Shafer - 2020

use super::*;
use crate::category5::input::Input;
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
            }
            // When focus changes between subsurfaces, we don't change the order. Only
            // wl_subsurface changes the order
            // set win to the surf focus
            self.set_surf_focus(win);
            // Send enter event(s) to the new focus
            // spec says this MUST be done after the leave events are sent
            Input::keyboard_enter(self, id);
        } else {
            // Clear the previous window focus from the skiplist
            if let Some(prev) = self.get_win_focus() {
                self.skiplist_remove_window(prev);
            }
            // Otherwise we have unselected any surfaces, so clear both focus types
            self.set_win_focus(None);
            self.set_surf_focus(None);
        }

        // TODO: recalculate skip
    }

    pub fn add_new_top_subsurf(&mut self, parent: WindowId, win: WindowId) {
        log::debug!("Adding subsurface {:?} to {:?}", win, parent);
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
    }

    /// The recursive portion of `map_on_surfs`
    fn find_window_at_point_recurse(&self, win: WindowId, func: FnMut(WindowId)) {
        // First recursively check all subsurfaces
        for sub in self.visible_subsurfaces(win) {
            self.find_window_at_point_recurse(sub, func);
            func(sub);
        }
    }

    /// Helper for walking the surface tree recursively.
    /// `func` will be called on every window
    pub fn map_on_surfs(&self, func: FnMut(WindowId)) {
        for win in self.visible_windows() {
            self.map_on_surf_tree_recurse(win, func);
            func(win);
        }
    }

    pub fn print_surface_tree(&self) {
        log::debug!("Dumping surface tree (front to back):");
        self.map_on_surfs(|win| {
            log::debug!("{:?}", win);
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
