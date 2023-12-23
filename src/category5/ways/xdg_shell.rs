// Implementation of the xdg_shell and xdg_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_protocols;
extern crate wayland_server as ws;

use wayland_protocols::xdg::shell::server::*;
use ws::protocol::wl_surface;
use ws::Resource;

use super::role::Role;
use super::surface::*;
use crate::category5::vkcomp::wm;
use crate::category5::Climate;

extern crate utils as cat5_utils;
use crate::category5::atmosphere::{Atmosphere, SurfaceId};
use cat5_utils::{log, region::Rect};

use std::clone::Clone;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

// --------------------------------------------------------------
// xdg_wm_base
// --------------------------------------------------------------

#[allow(unused_variables)]
impl ws::GlobalDispatch<xdg_wm_base::XdgWmBase, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<xdg_wm_base::XdgWmBase>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<xdg_wm_base::XdgWmBase, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &xdg_wm_base::XdgWmBase,
        request: xdg_wm_base::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        xdg_wm_base_handle_request(client, data_init, resource, request);
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &xdg_wm_base::XdgWmBase,
        data: &(),
    ) {
    }
}

/// Handle requests to a xdg_shell interface
///
/// The xdg_shell interface implements functionality regarding
/// the lifecycle of the window. Essentially it just creates
/// a xdg_shell_surface.
pub fn xdg_wm_base_handle_request(
    _client: &ws::Client,
    data_init: &mut ws::DataInit<'_, Climate>,
    _resource: &xdg_wm_base::XdgWmBase,
    req: xdg_wm_base::Request,
) {
    match req {
        xdg_wm_base::Request::GetXdgSurface { id, surface } => {
            // get category5's surface from the userdata
            let surf = surface.data::<Arc<Mutex<Surface>>>().unwrap();

            let shsurf = Arc::new(Mutex::new(ShellSurface {
                ss_surface: surf.clone(),
                ss_surface_proxy: surface,
                ss_serial: 0,
                ss_last_acked: 0,
                ss_xdg_toplevel: None,
                ss_xdg_popup: None,
            }));

            // Pass ourselves as user data
            data_init.init(id, shsurf);
        }
        xdg_wm_base::Request::CreatePositioner { id } => {
            let pos = Arc::new(Mutex::new(Positioner {
                p_offset: None,
                p_width: 0,
                p_height: 0,
                p_anchor_rect: Rect::new(0, 0, 0, 0),
                p_anchor: xdg_positioner::Anchor::None,
                p_gravity: xdg_positioner::Gravity::None,
                p_constraint: xdg_positioner::ConstraintAdjustment::None,
                p_reactive: false,
                p_parent_size: None,
                p_parent_configure: 0,
            }));

            // We will add the positioner as userdata since nothing
            // else needs to look it up
            data_init.init(id, pos);
        }
        xdg_wm_base::Request::Destroy => log::debug!("xdg_wm_base.destroy: impelementme"),
        _ => unimplemented!(),
    };
}

// --------------------------------------------------------------
// xdg_surface
// --------------------------------------------------------------

#[allow(unused_variables)]
impl ws::Dispatch<xdg_surface::XdgSurface, Arc<Mutex<ShellSurface>>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &xdg_surface::XdgSurface,
        request: xdg_surface::Request,
        data: &Arc<Mutex<ShellSurface>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        xdg_surface_handle_request(
            state.c_atmos.lock().unwrap().deref_mut(),
            client,
            data_init,
            resource,
            request,
        );
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &xdg_surface::XdgSurface,
        data: &Arc<Mutex<ShellSurface>>,
    ) {
    }
}

/// Handle requests to a xdg_shell_surface interface
///
/// xdg_shell_surface is the interface which actually
/// tracks window characteristics and roles. It is
/// highly recommended to read wayland.xml for all
/// the gory details.
fn xdg_surface_handle_request(
    atmos: &mut Atmosphere,
    _client: &ws::Client,
    data_init: &mut ws::DataInit<'_, Climate>,
    surf: &xdg_surface::XdgSurface,
    req: xdg_surface::Request,
) {
    // first clone the ShellSurface to be used as
    // userdata later
    let ss_clone = surf.data::<Arc<Mutex<ShellSurface>>>().unwrap().clone();

    // Now get a ref to the ShellSurface
    let mut shsurf = surf
        .data::<Arc<Mutex<ShellSurface>>>()
        .unwrap()
        .lock()
        .unwrap();

    match req {
        xdg_surface::Request::GetToplevel { id } => {
            let xdg = data_init.init(id, ss_clone.clone());
            shsurf.get_toplevel(atmos, surf, xdg, ss_clone)
        }
        xdg_surface::Request::GetPopup {
            id,
            parent,
            positioner,
        } => {
            let xdg = data_init.init(id, ss_clone.clone());
            shsurf.get_popup(atmos, xdg, surf, parent, positioner, ss_clone)
        }
        xdg_surface::Request::AckConfigure { serial } => {
            log::debug!("xdg_surface: client acked configure event {}", serial);
            shsurf.ack_configure(serial);
        }
        xdg_surface::Request::SetWindowGeometry {
            x,
            y,
            width,
            height,
        } => {
            log::debug!("xdg_surface: set geometry to:");
            log::debug!(
                "              x={} y={} width={} height={}",
                x,
                y,
                width,
                height
            );
            shsurf.set_win_geom(atmos, surf, x, y, width, height);
        }
        xdg_surface::Request::Destroy => (),
        _ => unimplemented!(),
    };
}

/// A shell surface
///
/// This is the private protocol object for
/// xdg_shell_surface. It actually implements
/// all the functionality that the handler
/// dispatches
#[allow(dead_code)]
pub struct ShellSurface {
    // Category5 surface state object
    ss_surface: Arc<Mutex<Surface>>,
    // the wayland proxy
    ss_surface_proxy: wl_surface::WlSurface,
    ss_xdg_toplevel: Option<xdg_toplevel::XdgToplevel>,
    ss_xdg_popup: Option<Popup>,
    // A serial for tracking config updates
    // every time we request a configuration this is the
    // serial number used
    pub ss_serial: u32,
    pub ss_last_acked: u32,
}

impl ShellSurface {
    /// Register a new popup surface.
    ///
    /// A popup surface is for dropdowns and alerts, and is the consumer
    /// of the positioner code. It is assigned a position over a parent
    /// toplevel surface and may exclusively grab input for that app.
    fn get_popup(
        &mut self,
        atmos: &mut Atmosphere,
        popup: xdg_popup::XdgPopup,
        xdg_surf: &xdg_surface::XdgSurface,
        parent: Option<xdg_surface::XdgSurface>,
        positioner: xdg_positioner::XdgPositioner,
        userdata: Arc<Mutex<ShellSurface>>,
    ) {
        self.ss_xdg_popup = Some(Popup {
            pu_pop: popup.clone(),
            pu_parent: parent.clone(),
        });

        {
            // assign the popup role
            let surf_cell = self.ss_surface.clone();
            let mut surf = surf_cell.lock().unwrap();
            let parent_surf = parent.as_ref().expect("Bug: popup missing parent");
            // Now get our ShellSurface object from the XdgSurface protocol object
            let parent_xdgsurf = parent_surf
                .data::<Arc<Mutex<ShellSurface>>>()
                .unwrap()
                .lock()
                .unwrap();
            let parent_surf_id = &parent_xdgsurf.ss_surface.lock().unwrap().s_id;

            // Now we can tell vkcomp to add this surface to the subsurface stack
            // in Thundr
            atmos.add_new_top_subsurf(parent_surf_id, &surf.s_id);
            log::debug!(
                "Adding popup subsurf {:?} to parent {:?}",
                surf.s_id.get_raw_id(),
                parent_surf_id,
            );

            surf.s_role = Some(Role::xdg_shell_popup(userdata.clone()));

            surf.s_state.cs_xdg_state.xs_popup_state = Some(PopupState {
                ps_positioner: positioner,
            });
            self.reposition_popup(&mut surf, xdg_surf);
        }
    }

    /// Generate a fresh set of configure events
    ///
    /// This is called from other subsystems (input), which means we need to
    /// pass the surface as an argument since its refcell will already be
    /// borrowed.
    pub fn configure(
        &mut self,
        atmos: &mut Atmosphere,
        xdg_surf: xdg_surface::XdgSurface,
        surf: &mut Surface,
        resizing: bool,
    ) {
        log::debug!("xdg_surface: generating configure event {}", self.ss_serial);
        let state = &mut surf.s_state.cs_xdg_state;

        // send configuration requests to the client
        if let Some(toplevel) = &self.ss_xdg_toplevel {
            let tlstate = state.xs_tlstate.as_mut().unwrap();

            // Get the current window size
            let mut size = if let Some(cur_size) = state.xs_size {
                // if the client manually requested a size, honor that
                cur_size
            } else {
                // If we don't have the size saved then grab the latest
                // from atmos.
                // If we have configured before then we should get the size
                // of the last one and update that.
                match tlstate.tl_cached_size {
                    (0, 0) => {
                        let raw_size = *atmos.a_window_size.get(&surf.s_id).unwrap();
                        (raw_size.0 as i32, raw_size.1 as i32)
                    }
                    tlsize => tlsize,
                }
            };

            if resizing {
                // Check that our cursor is not within the min size or exceeds the max
                // size before we apply this mouse diff. If we start adding mouse movements
                // to the size but we haven't returned to the edge then the resize doesn't
                // match the mouse position.
                let (cx, cy) = atmos.get_cursor_pos();
                // Adjust the cursor position to account for the menubar
                let (cx, cy) = atmos.get_adjusted_desktop_coord(cx as f32, cy as f32);
                let (cx, cy) = (cx as i32, cy as i32);
                log::debug!("cursor pos {:?}", (cx, cy));
                let wp = atmos.a_window_pos.get(&surf.s_id).unwrap();
                log::debug!("Window pos {:?}", *wp);
                let wp = (wp.0 as i32, wp.1 as i32);
                let ws = atmos.a_window_size.get(&surf.s_id).unwrap();
                let ws = (ws.0 as i32, ws.1 as i32);
                // TODO: subtrace size from pos if left or bottom_left?
                let min = tlstate.tl_min_size.unwrap_or((1, 1));
                let max = tlstate.tl_max_size.unwrap_or((i32::MAX, i32::MAX));

                // "c" will have to fall somewhere in these ranges to be valid
                //
                // For right edge growing:
                //    |------------|~~~~~~~~~~~~~~~~~~~~~~~~|
                //   pos          min                      max
                //
                // For left edge growing:
                //    |~~~~~~~~~~~~~~~~~~~~|~~~~~~~~~~|-----|
                //   max                  pos        min   edge
                let grow_mode_right = |c, pos: i32, min, max| {
                    c > pos + min && c < pos.checked_add(max).unwrap_or(i32::MAX)
                };
                let grow_mode_left = |c, min, max, edge: i32| {
                    c < edge - min && c > edge.checked_sub(max).unwrap_or(0)
                };

                // For edges that don't require position adjustment it is easy, we just
                // update the size if it passed the bounds check
                //
                // For this calculation we try to grow the size to where the cursor position
                // is, by finding the difference between the cursor and the surface start.
                if tlstate.tl_resize_right && grow_mode_right(cx, wp.0, min.0, max.0) {
                    // add but don't exceed the min/max bounds requested
                    size.0 = (cx - wp.0).clamp(min.0, max.0);
                }
                if tlstate.tl_resize_bottom && grow_mode_right(cy, wp.1, min.1, max.1) {
                    size.1 = (cy - wp.1).clamp(min.1, max.1);
                }

                // For edges that do require position adjustment we also have to calculate
                // a new position offset by our size change
                //
                // We do this by finding the bottom right edge of the surface by adding
                // the position and size. We can then subtract this by the cursor position
                // to find the new size to grow to.
                if tlstate.tl_resize_left && grow_mode_left(cx, min.0, max.0, wp.0 + ws.0) {
                    size.0 = ((wp.0 + ws.0) - cx).clamp(min.0, max.0);
                }
                if tlstate.tl_resize_top && grow_mode_left(cy, min.1, max.1, wp.1 + ws.1) {
                    size.1 = ((wp.1 + ws.1) - cy).clamp(min.1, max.1);
                }

                log::debug!("Resized to {:?}", size);
            }

            // build an array of state flags to pass to toplevel.configure
            let mut states: Vec<u8> = Vec::new();
            if tlstate.tl_maximized {
                states.push(xdg_toplevel::State::Maximized as u8);
            }
            if tlstate.tl_resizing {
                states.push(xdg_toplevel::State::Resizing as u8);
            }
            if tlstate.tl_fullscreen {
                states.push(xdg_toplevel::State::Fullscreen as u8);
            }
            log::debug!("xdg_surface: sending states {:?}", states);

            tlstate.tl_cached_size = (size.0, size.1);

            // send them to the client
            toplevel.configure(size.0 as i32, size.1 as i32, states);
        }

        xdg_surf.configure(self.ss_serial);
        self.ss_serial += 1;
    }

    /// Check if this serial is the currently loaned out one,
    /// and if so set the existing state to be applied
    pub fn ack_configure(&mut self, serial: u32) {
        // increment the serial for next timme
        self.ss_last_acked = serial;
    }

    /// Set the window geometry for this surface
    ///
    /// ???: According to the spec:
    ///     When maintaining a position, the compositor should treat the (x, y)
    ///     coordinate of the window geometry as the top left corner of the window.
    ///     A client changing the (x, y) window geometry coordinate should in
    ///     general not alter the position of the window.
    ///
    /// I think this means to just ignore x and y, and handle movement elsewhere
    fn set_win_geom(
        &mut self,
        atmos: &mut Atmosphere,
        _xdg_surf: &xdg_surface::XdgSurface,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) {
        let surf_cell = self.ss_surface.clone();
        let mut surf = surf_cell.lock().unwrap();

        // we need to update the *window* position
        // to be an offset from the base surface position
        let mut surf_pos = *atmos.a_surface_pos.get(&surf.s_id).unwrap();
        surf_pos.0 += x as f32;
        surf_pos.1 += y as f32;
        atmos.a_window_pos.set(&surf.s_id, surf_pos);

        surf.s_state.cs_xdg_state.xs_size = Some((width, height));
    }

    /// Get a toplevel surface
    ///
    /// A toplevel surface is the "normal" window type. It
    /// represents displaying one wl_surface in the desktop shell.
    /// `userdata` is a Rc ref of ourselves which should
    /// be added to toplevel.
    fn get_toplevel(
        &mut self,
        atmos: &mut Atmosphere,
        xdg_surf: &xdg_surface::XdgSurface,
        toplevel: xdg_toplevel::XdgToplevel,
        userdata: Arc<Mutex<ShellSurface>>,
    ) {
        // Mark our surface as being a window handled by xdg_shell
        let mut surf = self.ss_surface.lock().unwrap();
        surf.s_role = Some(Role::xdg_shell_toplevel(xdg_surf.clone(), userdata.clone()));
        surf.s_state.cs_xdg_state.xs_tlstate = Some(ToplevelState::empty());

        // Tell vkcomp to create a new window
        log::debug!("Setting surface {:?} to toplevel", surf.s_id.get_raw_id());
        atmos.a_toplevel.set(&surf.s_id, true);
        atmos.add_wm_task(wm::task::Task::new_toplevel(surf.s_id.clone()));
        // This places the surface at the front of the skiplist, aka
        // makes it in focus
        atmos.focus_on(Some(surf.s_id.clone()));

        // send configuration requests to the client
        // width and height 0 means client picks a size
        toplevel.configure(
            0,
            0,
            Vec::new(), // TODO: specify our requirements?
        );
        xdg_surf.configure(self.ss_serial);
        self.ss_serial += 1;

        // Now add ourselves to the xdg_toplevel
        self.ss_xdg_toplevel = Some(toplevel.clone());
    }
}

/// The xdg_toplevel state.
///
/// This contains basic information about the sizing of a surface.
/// It is tracked on a per configuration serial basis.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ToplevelState {
    /// window title
    pub tl_title: Option<String>,
    pub tl_app_id: Option<String>,
    // Is this window against a tile boundary?
    pub tl_resize_left: bool,
    pub tl_resize_right: bool,
    pub tl_resize_top: bool,
    pub tl_resize_bottom: bool,
    /// bounding dimensions
    pub tl_max_size: Option<(i32, i32)>,
    pub tl_min_size: Option<(i32, i32)>,
    // self-explanitory I think
    pub tl_maximized: bool,
    pub tl_minimized: bool,
    pub tl_fullscreen: bool,
    /// Is the window currently in focus?
    pub tl_activated: bool,
    pub tl_resizing: bool,
    /// The latest size used during configure. This doesn't
    /// actually control any commit behavior, but it is how we
    /// figure out the right size to recommend to our client
    /// every time we call configure.
    tl_cached_size: (i32, i32),
}

impl ToplevelState {
    /// Return a state with no changes
    fn empty() -> ToplevelState {
        ToplevelState {
            tl_maximized: false,
            tl_fullscreen: false,
            tl_minimized: false,
            tl_activated: false,
            tl_resizing: false,
            tl_cached_size: (0, 0),
            tl_title: None,
            tl_app_id: None,
            tl_resize_left: false,
            tl_resize_right: false,
            tl_resize_top: false,
            tl_resize_bottom: false,
            tl_max_size: None,
            tl_min_size: None,
        }
    }

    fn commit(&mut self, surf_id: &SurfaceId, atmos: &mut Atmosphere, size_diff: (f32, f32)) {
        // If we are resizing the left or top, then we need to offset
        // our window position by the change in size
        if (self.tl_resize_left || self.tl_resize_top) && size_diff != (0.0, 0.0) {
            // We need to update the window size.
            //
            // The surface pos controlls where this surface actually
            // is in the desktop. The window pos is a section inside
            // of the surface where content is displayed
            let mut sp = atmos.a_surface_pos.get_mut(surf_id).unwrap();
            let mut wp = atmos.a_window_pos.get_mut(surf_id).unwrap();
            log::debug!("Old surface pos is {:?}", *sp);
            if self.tl_resize_left {
                sp.0 -= size_diff.0;
                wp.0 -= size_diff.0;
            }
            if self.tl_resize_top {
                sp.1 -= size_diff.1;
                wp.1 -= size_diff.1;
            }
            log::debug!("New surface pos is {:?}", *sp);
        }

        // TODO: handle min/max/fullscreen/activated
    }
}

/// Surface state for xdg_popup surfaces
///
/// This really just holds a positioner which will be used to
/// recalculate the surface size/pos during a commit.
#[derive(Clone)]
pub struct PopupState {
    /// Positioner for popup surface
    ps_positioner: xdg_positioner::XdgPositioner,
}

impl PopupState {
    fn commit(&mut self, surf_id: &SurfaceId, atmos: &mut Atmosphere) {
        // Update the size and position from the latest reposition
        let pos_cell = self
            .ps_positioner
            .data::<Arc<Mutex<Positioner>>>()
            .expect("Bug: positioner did not have userdata attached")
            .clone();
        let positioner = pos_cell.lock().unwrap();

        let pos_loc = positioner.get_loc();
        atmos
            .a_surface_pos
            .set(surf_id, (pos_loc.0 as f32, pos_loc.1 as f32));
        atmos.a_window_size.set(
            surf_id,
            (positioner.p_width as f32, positioner.p_height as f32),
        );
    }
}

/// This is the set of outstanding
/// configuration changes which have not
/// been committed to the atmos yet.
///
/// Configuration is a bit weird, I think it looks like:
/// first: any role interfaces will send their own configure events, which
///   request the client to set itself to match a certain state (size, maximized, etc)
/// second: once that is done, the xdg_wm_base will send a configure event
///   saying that the configuration requests are over with.
/// thirdish: the client will start making requests that update each part
///   of the window state (i.e. set the size/title)
/// fourth: the client will do the ack_configure request to tell the server
///   that it is done.
/// finally: the client will commit the surface, causing the server to apply
///   all of the attached state
#[allow(dead_code)]
pub struct XdgState {
    /// The current toplevel state
    pub xs_tlstate: Option<ToplevelState>,
    /// The current xdg_popup state
    pub xs_popup_state: Option<PopupState>,
    /// Window position
    pub xs_size: Option<(i32, i32)>,
}

impl XdgState {
    /// Return a state with no changes
    pub fn empty() -> XdgState {
        XdgState {
            xs_tlstate: None,
            xs_popup_state: None,
            xs_size: None,
        }
    }

    pub fn clone_refresh(&mut self) -> Self {
        Self {
            xs_tlstate: self.xs_tlstate.clone(),
            xs_popup_state: self.xs_popup_state.clone(),
            xs_size: self.xs_size.take(),
        }
    }

    pub fn commit(&mut self, surf_id: &SurfaceId, atmos: &mut Atmosphere) {
        log::debug!("xdg_surface.commit");

        // Set our window size
        let ws = match atmos.a_window_size.get(surf_id).map(|ws| *ws) {
            Some(ws) => ws,
            None => {
                // If this is the first toplevel commit then we need
                // to set our window size for the first time.
                //
                // The client should have updated the window geometry in reaction
                // to the last acked configure event. If it doesn't exist then
                // just set the window size to the surface size as per the spec
                let ws = match self.xs_size {
                    Some((w, h)) => (w as f32, h as f32),
                    None => *atmos.a_surface_size.get(surf_id).unwrap(),
                };
                atmos.a_window_size.set(surf_id, ws);
                ws
            }
        };
        let size = match self.xs_size {
            Some((w, h)) => (w as f32, h as f32),
            None => ws,
        };
        let size_diff = (size.0 - ws.0, size.1 - ws.1);

        atmos.a_window_size.set(surf_id, size);

        self.xs_size = None;

        if let Some(tlstate) = self.xs_tlstate.as_mut() {
            tlstate.commit(surf_id, atmos, size_diff);
        }

        if let Some(popstate) = self.xs_popup_state.as_mut() {
            popstate.commit(surf_id, atmos);
        }
    }
}

// --------------------------------------------------------------
// xdg_toplevel
// --------------------------------------------------------------

#[allow(unused_variables)]
impl ws::Dispatch<xdg_toplevel::XdgToplevel, Arc<Mutex<ShellSurface>>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &xdg_toplevel::XdgToplevel,
        request: xdg_toplevel::Request,
        data: &Arc<Mutex<ShellSurface>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data.lock().unwrap().handle_toplevel_request(
            state.c_atmos.lock().unwrap().deref_mut(),
            client,
            data_init,
            resource,
            request,
        );
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &xdg_toplevel::XdgToplevel,
        data: &Arc<Mutex<ShellSurface>>,
    ) {
    }
}

impl ShellSurface {
    /// handle xdg_toplevel requests
    ///
    /// This should load our xdg state with any changes that the client
    /// has made, and they will be applied during the next commit.
    fn handle_toplevel_request(
        &mut self,
        atmos: &mut Atmosphere,
        _client: &ws::Client,
        _data_init: &mut ws::DataInit<'_, Climate>,
        _toplevel: &xdg_toplevel::XdgToplevel,
        req: xdg_toplevel::Request,
    ) {
        let mut surf = self.ss_surface.lock().unwrap();
        let id = surf.s_id.clone();
        let tl = surf.s_state.cs_xdg_state.xs_tlstate.as_mut().unwrap();

        #[allow(unused_variables)]
        match req {
            xdg_toplevel::Request::Destroy => (),
            xdg_toplevel::Request::SetParent { parent } => (),
            xdg_toplevel::Request::SetTitle { title } => tl.tl_title = Some(title),
            xdg_toplevel::Request::SetAppId { app_id } => tl.tl_app_id = Some(app_id),
            xdg_toplevel::Request::ShowWindowMenu { seat, serial, x, y } => (),
            xdg_toplevel::Request::Move { seat, serial } => {
                // Moving is NOT double buffered so just grab it now
                let id = surf.s_id.clone();
                atmos.set_grabbed(Some(id));
            }
            xdg_toplevel::Request::Resize {
                seat,
                serial,
                edges,
            } => {
                // Moving is NOT double buffered so just grab it now
                atmos.set_resizing(Some(id));
                (
                    tl.tl_resize_right,
                    tl.tl_resize_left,
                    tl.tl_resize_top,
                    tl.tl_resize_bottom,
                ) = match edges.into_result().expect("Invalid resize edge flag") {
                    xdg_toplevel::ResizeEdge::Right => (true, false, false, false),
                    xdg_toplevel::ResizeEdge::Left => (false, true, false, false),
                    xdg_toplevel::ResizeEdge::Top => (false, false, true, false),
                    xdg_toplevel::ResizeEdge::Bottom => (false, false, false, true),
                    xdg_toplevel::ResizeEdge::TopRight => (true, false, true, false),
                    xdg_toplevel::ResizeEdge::BottomRight => (true, false, false, true),
                    xdg_toplevel::ResizeEdge::TopLeft => (false, true, true, false),
                    xdg_toplevel::ResizeEdge::BottomLeft => (false, true, false, true),
                    _ => (false, false, false, false),
                };
                tl.tl_resizing = true;
            }
            xdg_toplevel::Request::SetMaxSize { width, height } => {
                tl.tl_max_size = Some((width, height))
            }
            xdg_toplevel::Request::SetMinSize { width, height } => {
                tl.tl_min_size = Some((width, height))
            }
            xdg_toplevel::Request::SetMaximized => tl.tl_maximized = true,
            xdg_toplevel::Request::UnsetMaximized => tl.tl_maximized = false,
            xdg_toplevel::Request::SetFullscreen { output } => tl.tl_fullscreen = true,
            xdg_toplevel::Request::UnsetFullscreen => tl.tl_fullscreen = false,
            xdg_toplevel::Request::SetMinimized => tl.tl_minimized = true,
            _ => unimplemented!(),
        }
    }
}

// --------------------------------------------------------------
// xdg_positioner
// --------------------------------------------------------------

#[allow(unused_variables)]
impl ws::Dispatch<xdg_positioner::XdgPositioner, Arc<Mutex<Positioner>>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &xdg_positioner::XdgPositioner,
        request: xdg_positioner::Request,
        data: &Arc<Mutex<Positioner>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        xdg_positioner_handle_request(resource, request);
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &xdg_positioner::XdgPositioner,
        data: &Arc<Mutex<Positioner>>,
    ) {
    }
}

/// Respond to xdg_positioner requests.
///
/// These requests are used to build up a `Positioner`, which will
/// later be used during the creation of an `xdg_popup` surface.
fn xdg_positioner_handle_request(
    res: &xdg_positioner::XdgPositioner,
    req: xdg_positioner::Request,
) {
    let pos_cell = res
        .data::<Arc<Mutex<Positioner>>>()
        .expect("xdg_positioner did not contain the correct userdata")
        .clone();
    let mut pos = pos_cell.lock().unwrap();

    // add the reqeust data to our struct
    match req {
        xdg_positioner::Request::SetSize { width, height } => {
            pos.p_width = width;
            pos.p_height = height;
        }
        xdg_positioner::Request::SetAnchorRect {
            x,
            y,
            width,
            height,
        } => {
            pos.p_anchor_rect = Rect::new(x, y, width, height);
        }
        xdg_positioner::Request::SetAnchor { anchor } => {
            pos.p_anchor = anchor.into_result().unwrap()
        }
        xdg_positioner::Request::SetGravity { gravity } => {
            pos.p_gravity = gravity.into_result().unwrap()
        }
        xdg_positioner::Request::SetConstraintAdjustment {
            constraint_adjustment,
        } => {
            pos.p_constraint =
                xdg_positioner::ConstraintAdjustment::from_bits(constraint_adjustment).unwrap()
        }
        xdg_positioner::Request::SetOffset { x, y } => {
            pos.p_offset = Some((x, y));
        }
        xdg_positioner::Request::SetReactive => pos.p_reactive = true,
        xdg_positioner::Request::SetParentSize {
            parent_width,
            parent_height,
        } => pos.p_parent_size = Some((parent_width, parent_height)),
        xdg_positioner::Request::SetParentConfigure { serial } => pos.p_parent_configure = serial,
        xdg_positioner::Request::Destroy => (),
        _ => unimplemented!(),
    };
}

/// A positioner object
///
/// This is used to position popups relative to the toplevel parent
/// surface. It handles offsets and anchors for hovering the popup
/// surface.
///
/// For a
#[derive(Copy, Clone)]
struct Positioner {
    /// The offset, as set by `set_offset`
    p_offset: Option<(i32, i32)>,
    /// The positioner dimensions, as set by `set_size`
    p_width: i32, // from set_size
    p_height: i32,
    // (x, y, width, height) of the anchor rectangle
    p_anchor_rect: Rect<i32>,
    p_anchor: xdg_positioner::Anchor,
    p_gravity: xdg_positioner::Gravity,
    p_constraint: xdg_positioner::ConstraintAdjustment,
    // If the constraints should be recalculated when the parent is moved
    p_reactive: bool,
    p_parent_size: Option<(i32, i32)>,
    /// The serial of the parent configuration event this is responding to
    p_parent_configure: u32,
}

impl Positioner {
    /// Create a surface local position from the positioner.
    /// This should be called to reevaluate the end result of the popup location.
    fn get_loc(&self) -> (i32, i32) {
        // The spec states that we MUST have a non-zero anchor rect, and a size
        let mut ret = (0, 0);

        // First start off with an offset, if we have one
        if let Some((x, y)) = self.p_offset {
            ret.0 += x;
            ret.1 += y;
        }

        // Now add in the anchor rectangle and the anchor point
        //
        // The anchor rectangle will be positioned somewhere inside the parent. We then
        // need to calculate a position on one of the edges of the anchor rectangle. Unless
        // none is specified, we add the offset to the anchor rect's top left, then add some
        // more based on the size to match the position on the edge that was requested.
        let rect = &self.p_anchor_rect;
        let anchor_offset = match self.p_anchor {
            xdg_positioner::Anchor::None => (0, 0),
            xdg_positioner::Anchor::Top => (rect.r_pos.0 + rect.r_size.0 / 2, rect.r_pos.1),
            xdg_positioner::Anchor::Bottom => (
                rect.r_pos.0 + rect.r_size.0 / 2,
                rect.r_pos.1 + rect.r_size.1,
            ),
            xdg_positioner::Anchor::Left => (rect.r_pos.0, rect.r_pos.1 + rect.r_size.1 / 2),
            xdg_positioner::Anchor::Right => (
                rect.r_pos.0 + rect.r_size.0,
                rect.r_pos.1 + rect.r_size.1 / 2,
            ),
            xdg_positioner::Anchor::TopLeft => (rect.r_pos.0, rect.r_pos.1),
            xdg_positioner::Anchor::BottomLeft => (rect.r_pos.0, rect.r_pos.1 + rect.r_size.1),
            xdg_positioner::Anchor::TopRight => (rect.r_pos.0 + rect.r_size.0, rect.r_pos.1),
            xdg_positioner::Anchor::BottomRight => {
                (rect.r_pos.0 + rect.r_size.0, rect.r_pos.1 + rect.r_size.1)
            }
            _ => unimplemented!(),
        };
        ret.0 += anchor_offset.0;
        ret.1 += anchor_offset.1;

        // TODO: subtract on each side to handle gravity

        // TODO: add the rest of the positioner elements
        return ret;
    }
}

// --------------------------------------------------------------
// xdg_popup
// --------------------------------------------------------------

#[allow(unused_variables)]
impl ws::Dispatch<xdg_popup::XdgPopup, Arc<Mutex<ShellSurface>>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &xdg_popup::XdgPopup,
        request: xdg_popup::Request,
        data: &Arc<Mutex<ShellSurface>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data.lock().unwrap().handle_popup_request(
            state.c_atmos.lock().unwrap().deref_mut(),
            client,
            data_init,
            resource,
            request,
        );
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &xdg_popup::XdgPopup,
        data: &Arc<Mutex<ShellSurface>>,
    ) {
    }
}

/// Private struct for an xdg_popup role.
#[allow(dead_code)]
pub struct Popup {
    pu_pop: xdg_popup::XdgPopup,
    pu_parent: Option<xdg_surface::XdgSurface>,
}

impl ShellSurface {
    /// Calculate the position for this popup, and generate configure
    /// events broadcasting it.
    /// This will use the repositioned value if it was set.
    fn reposition_popup(&mut self, surf: &mut Surface, xdg_surf: &xdg_surface::XdgSurface) {
        let pop = self.ss_xdg_popup.as_mut().unwrap();
        let state = surf.s_state.cs_xdg_state.xs_popup_state.as_mut().unwrap();

        let pos_cell = state
            .ps_positioner
            .data::<Arc<Mutex<Positioner>>>()
            .expect("Bug: positioner did not have userdata attached")
            .clone();
        let pos = pos_cell.lock().unwrap();

        // send configuration requests to the client
        // width and height 0 means client picks a size
        let popup_loc = pos.get_loc();
        log::debug!("Popup location: {:?}", popup_loc);
        pop.pu_pop
            .configure(popup_loc.0, popup_loc.1, pos.p_width, pos.p_height);
        xdg_surf.configure(self.ss_serial);
        self.ss_serial += 1;
    }

    fn popup_done(&mut self, _atmos: &mut Atmosphere) {
        self.ss_surface.lock().unwrap().s_role = None;
        self.ss_xdg_popup.take().unwrap().pu_pop.popup_done();
    }

    /// handle xdg_popup requests
    ///
    /// This should load our xdg state with any changes that the client
    /// has made, and they will be applied during the next commit.
    /// There is relatively little compared to xdg_toplevel.
    fn handle_popup_request(
        &mut self,
        atmos: &mut Atmosphere,
        _client: &ws::Client,
        _data_init: &mut ws::DataInit<'_, Climate>,
        _popup: &xdg_popup::XdgPopup,
        req: xdg_popup::Request,
    ) {
        // TODO: implement the remaining handlers
        #[allow(unused_variables)]
        match req {
            xdg_popup::Request::Destroy => {
                log::debug!("Popup destroyed. Dismissing it");
                self.popup_done(atmos);
            }
            // TODO: implement grab
            xdg_popup::Request::Grab { seat, serial } => {
                log::debug!("Grabbing a popup is not supported");
                self.popup_done(atmos);
            }
            xdg_popup::Request::Reposition { positioner, token } => {
                let mut surf = self.ss_surface.lock().unwrap();
                let state = surf.s_state.cs_xdg_state.xs_popup_state.as_mut().unwrap();
                state.ps_positioner = positioner;
            }
            _ => unimplemented!(),
        }
    }
}
