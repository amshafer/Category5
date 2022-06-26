// Implementation of the xdg_shell and xdg_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::protocol::wl_surface;
use ws::Main;

pub use super::protocol::xdg_shell::*;
use super::role::Role;
use super::surface::*;

extern crate utils as cat5_utils;
use crate::category5::atmosphere::Atmosphere;
use cat5_utils::log;

use std::cell::RefCell;
use std::clone::Clone;
use std::rc::Rc;

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
    pub xs_pos: Option<(i32, i32)>,
    pub xs_size: Option<(i32, i32)>,
    // window title
    pub xs_title: Option<String>,
    pub xs_app_id: Option<String>,
    // Is this window against a tile boundary?
    pub xs_tiled_left: bool,
    pub xs_tiled_right: bool,
    pub xs_tiled_top: bool,
    pub xs_tiled_bottom: bool,
    // bounding dimensions
    // (0, 0) means it has not yet been set
    pub xs_max_size: Option<(i32, i32)>,
    pub xs_min_size: Option<(i32, i32)>,

    // ------------------
    // The following are "meta" configuration changes
    // aka making new role objects, not related to the
    // window itself
    // ------------------
    // Should we create a new window
    xs_make_new_toplevel_window: bool,
    xs_make_new_popup_window: bool,
    xs_moving: bool,
    pub xs_acked: bool,
}

impl XdgState {
    /// Return a state with no changes
    fn empty() -> XdgState {
        XdgState {
            xs_pos: None,
            xs_size: None,
            xs_acked: false,
            xs_title: None,
            xs_app_id: None,
            xs_tiled_left: false,
            xs_tiled_right: false,
            xs_tiled_top: false,
            xs_tiled_bottom: false,
            xs_max_size: None,
            xs_min_size: None,
            xs_make_new_toplevel_window: false,
            xs_make_new_popup_window: false,
            xs_moving: false,
        }
    }
}

/// The xdg_toplevel state.
///
/// This contains basic information about the sizing of a surface.
/// It is tracked on a per configuration serial basis.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct TLState {
    // self-explanitory I think
    pub tl_maximized: bool,
    pub tl_minimized: bool,
    // guess what this one means
    pub tl_fullscreen: bool,
    // Is the window currently in focus?
    pub tl_activated: bool,
    pub tl_resizing: bool,
}

impl TLState {
    /// Return a state with no changes
    fn empty() -> TLState {
        TLState {
            tl_maximized: false,
            tl_fullscreen: false,
            tl_minimized: false,
            tl_activated: false,
            tl_resizing: false,
        }
    }
}

/// A complete xdg_toplevel configuration
/// This pairs the above toplevel state with a serial range
/// that is holds true for
#[derive(Debug)]
struct TLConfig {
    /// The serial numbers that this state describes
    tlc_serial: u32,
    /// The size of the window.
    /// When the client acks a configure event we will look up
    /// the TLConfig for that serial, and update the window
    /// size to this in `commit`.
    tlc_size: (i32, i32),
    /// reference count this to avoid extra allocations
    tlc_state: Rc<TLState>,
}

impl TLConfig {
    fn new(serial: u32, width: i32, height: i32, state: Rc<TLState>) -> TLConfig {
        TLConfig {
            tlc_serial: serial,
            tlc_size: (width, height),
            tlc_state: state,
        }
    }
}

/// Handle requests to a xdg_shell interface
///
/// The xdg_shell interface implements functionality regarding
/// the lifecycle of the window. Essentially it just creates
/// a xdg_shell_surface.
pub fn xdg_wm_base_handle_request(
    req: xdg_wm_base::Request,
    _wm_base: Main<xdg_wm_base::XdgWmBase>,
) {
    match req {
        xdg_wm_base::Request::GetXdgSurface { id: xdg, surface } => {
            // get category5's surface from the userdata
            let surf = surface
                .as_ref()
                .user_data()
                .get::<Rc<RefCell<Surface>>>()
                .unwrap();
            let atmos = surf.borrow_mut().s_atmos.clone();

            let shsurf = Rc::new(RefCell::new(ShellSurface {
                ss_atmos: atmos.clone(),
                ss_surface: surf.clone(),
                ss_surface_proxy: surface,
                ss_xdg_surface: xdg.clone(),
                ss_xs: XdgState::empty(),
                ss_serial: 0,
                ss_last_acked: 0,
                ss_xdg_toplevel: None,
                ss_xdg_popup: None,
                ss_cur_tlstate: TLState::empty(),
                ss_tlconfigs: Vec::new(),
            }));

            xdg.quick_assign(|s, r, _| {
                xdg_surface_handle_request(s, r);
            });
            // Pass ourselves as user data
            xdg.as_ref().user_data().set(move || shsurf);
        }
        xdg_wm_base::Request::CreatePositioner { id } => {
            let pos = Rc::new(RefCell::new(Positioner {
                p_offset: None,
                p_width: 0,
                p_height: 0,
                p_anchor_rect: (0, 0, 0, 0),
                p_anchor: xdg_positioner::Anchor::None,
                p_gravity: xdg_positioner::Gravity::None,
                p_constraint: xdg_positioner::ConstraintAdjustment::None,
                p_reactive: false,
                p_parent_size: None,
                p_parent_configure: 0,
            }));

            id.quick_assign(|s, r, _| {
                xdg_positioner_handle_request(s, r);
            });
            // We will add the positioner as userdata since nothing
            // else needs to look it up
            id.as_ref().user_data().set(move || pos);
        }
        xdg_wm_base::Request::Destroy => log::debug!("xdg_wm_base.destroy: impelementme"),
        _ => unimplemented!(),
    };
}

/// Handle requests to a xdg_shell_surface interface
///
/// xdg_shell_surface is the interface which actually
/// tracks window characteristics and roles. It is
/// highly recommended to read wayland.xml for all
/// the gory details.
fn xdg_surface_handle_request(surf: Main<xdg_surface::XdgSurface>, req: xdg_surface::Request) {
    // first clone the ShellSurface to be used as
    // userdata later
    let ss_clone = surf
        .as_ref()
        .user_data()
        .get::<Rc<RefCell<ShellSurface>>>()
        .unwrap()
        .clone();

    // Now get a ref to the ShellSurface
    let mut shsurf = surf
        .as_ref()
        .user_data()
        .get::<Rc<RefCell<ShellSurface>>>()
        .unwrap()
        .borrow_mut();

    match req {
        xdg_surface::Request::GetToplevel { id: xdg } => shsurf.get_toplevel(xdg, ss_clone),
        xdg_surface::Request::GetPopup {
            id,
            parent,
            positioner,
        } => shsurf.get_popup(id, parent, positioner, ss_clone),
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
            shsurf.set_win_geom(x, y, width, height);
        }
        xdg_surface::Request::Destroy => (),
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
    p_anchor_rect: (i32, i32, i32, i32),
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
        let mut ret = (self.p_anchor_rect.0, self.p_anchor_rect.1);

        if let Some((x, y)) = self.p_offset {
            ret.0 += x;
            ret.1 += y;
        }

        // TODO: add the rest of the positioner elements
        return ret;
    }
}

/// Respond to xdg_positioner requests.
///
/// These requests are used to build up a `Positioner`, which will
/// later be used during the creation of an `xdg_popup` surface.
fn xdg_positioner_handle_request(
    res: Main<xdg_positioner::XdgPositioner>,
    req: xdg_positioner::Request,
) {
    let pos_cell = res
        .as_ref()
        .user_data()
        .get::<Rc<RefCell<Positioner>>>()
        .expect("xdg_positioner did not contain the correct userdata")
        .clone();
    let mut pos = pos_cell.borrow_mut();

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
            pos.p_anchor_rect = (x, y, width, height);
        }
        xdg_positioner::Request::SetAnchor { anchor } => pos.p_anchor = anchor,
        xdg_positioner::Request::SetGravity { gravity } => pos.p_gravity = gravity,
        xdg_positioner::Request::SetConstraintAdjustment {
            constraint_adjustment,
        } => {
            pos.p_constraint =
                xdg_positioner::ConstraintAdjustment::from_raw(constraint_adjustment).unwrap()
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
    };
}

/// Private struct for an xdg_popup role.
#[allow(dead_code)]
pub struct Popup {
    pu_pop: Main<xdg_popup::XdgPopup>,
    pu_parent: Option<xdg_surface::XdgSurface>,
    pu_positioner: xdg_positioner::XdgPositioner,
    pu_next_positioner: Option<xdg_positioner::XdgPositioner>,
    /// A list of reposition requests. Spec states that if multiple
    /// are sent only the last one needs to be used.
    pu_reposition: Option<xdg_positioner::XdgPositioner>,
}

impl Popup {
    fn commit(&mut self, surf: &Surface, atmos: &mut Atmosphere, make_new_window: bool) {
        if make_new_window {
            log::debug!("Setting surface {:?} to popup", surf.s_id);
            // first get our parent surface
            let parent_surf = self
                .pu_parent
                .as_ref()
                .expect("Bug: popup did not have a parent assigned yet");
            // Now get our ShellSurface object from the XdgSurface protocol object
            let shsurf = parent_surf
                .as_ref()
                .user_data()
                .get::<Rc<RefCell<ShellSurface>>>()
                .unwrap()
                .borrow();

            // Now we can tell vkcomp to add this surface to the subsurface stack
            // in Thundr
            atmos.add_new_top_subsurf(shsurf.ss_surface.borrow().s_id, surf.s_id);
            log::error!(
                "Adding popup subsurf {:?} to parent {:?}",
                surf.s_id,
                shsurf.ss_surface.borrow().s_id
            );
        }

        // Update the size and position from the latest reposition
        let pos_cell = self
            .pu_positioner
            .as_ref()
            .user_data()
            .get::<Rc<RefCell<Positioner>>>()
            .expect("Bug: positioner did not have userdata attached")
            .clone();
        let positioner = pos_cell.borrow();

        let pos_loc = positioner.get_loc();
        atmos.set_surface_pos(surf.s_id, pos_loc.0 as f32, pos_loc.1 as f32);
        atmos.set_window_size(
            surf.s_id,
            positioner.p_width as f32,
            positioner.p_height as f32,
        );
    }
}

/// A shell surface
///
/// This is the private protocol object for
/// xdg_shell_surface. It actually implements
/// all the functionality that the handler
/// dispatches
#[allow(dead_code)]
pub struct ShellSurface {
    ss_atmos: Rc<RefCell<Atmosphere>>,
    // Category5 surface state object
    ss_surface: Rc<RefCell<Surface>>,
    // the wayland proxy
    ss_surface_proxy: wl_surface::WlSurface,
    // The object this belongs to
    ss_xdg_surface: Main<xdg_surface::XdgSurface>,
    ss_xdg_toplevel: Option<Main<xdg_toplevel::XdgToplevel>>,
    ss_xdg_popup: Option<Popup>,
    // Outstanding changes to be applied in commit
    pub ss_xs: XdgState,
    // A serial for tracking config updates
    // every time we request a configuration this is the
    // serial number used
    pub ss_serial: u32,
    pub ss_last_acked: u32,
    // The current toplevel state
    // This will get snapshotted and recorded for each config serial
    pub ss_cur_tlstate: TLState,
    // The list of pending configuration events
    ss_tlconfigs: Vec<TLConfig>,
}

impl ShellSurface {
    /// Surface is the caller, and it has already called
    /// borrow_mut, so it will just pass itself to us
    /// to prevent causing a refcell panic.
    /// The same goes for the atmosphere
    pub fn commit(&mut self, surf: &Surface, atmos: &mut Atmosphere) {
        // do nothing if the client has yet to ack these changes
        if !self.ss_xs.xs_acked {
            return;
        }

        // This has just been assigned role of toplevel
        if self.ss_xs.xs_make_new_toplevel_window {
            // Tell vkcomp to create a new window
            log::debug!("Setting surface {:?} to toplevel", surf.s_id);
            atmos.set_toplevel(surf.s_id, true);
            // This places the surface at the front of the skiplist, aka
            // makes it in focus
            atmos.focus_on(Some(surf.s_id));
            self.ss_xs.xs_make_new_toplevel_window = false;
        }

        if self.ss_xs.xs_moving {
            log::debug!("Moving surface {:?}", surf.s_id);
            atmos.set_grabbed(Some(surf.s_id));
            self.ss_xs.xs_moving = false;
        }

        // Handle popup surface updates
        if let Some(popup) = self.ss_xdg_popup.as_mut() {
            popup.commit(surf, atmos, self.ss_xs.xs_make_new_popup_window);

            // clear our double buffered state
            self.ss_xs.xs_make_new_popup_window = false;
        } else if let Some((i, tlc)) = self
            // find the toplevel state for the last config event acked
            // ack the toplevel configuration
            .ss_tlconfigs
            .iter()
            .enumerate()
            // Find the config which matches this serial
            .find(|&(_, tlc)| {
                if tlc.tlc_serial == self.ss_last_acked {
                    return true;
                }
                return false;
            })
        {
            // TODO: handle min/max/fullscreen/activated

            log::debug!("xdg_surface.commit: (ev {}) vvv", tlc.tlc_serial);

            // use the size from the latest acked config event
            let mut size = (tlc.tlc_size.0 as f32, tlc.tlc_size.1 as f32);
            // UNLESS the window geom was manually set, then we need to
            // honor that and use the double buffered value
            if let Some((w, h)) = self.ss_xs.xs_size {
                size = (w as f32, h as f32);
            }

            atmos.set_window_size(surf.s_id, size.0, size.1);

            self.ss_xs.xs_size = None;
            // remove all the previous/outdated configs
            self.ss_tlconfigs.drain(0..i);
        }

        // TODO: handle the other state changes
        //       make them options??

        // unset the ack for next time
        self.ss_xs.xs_acked = false;
    }

    /// Generate a fresh set of configure events
    ///
    /// This is called from other subsystems (input), which means we need to
    /// pass the surface as an argument since its refcell will already be
    /// borrowed.
    pub fn configure(
        &mut self,
        atmos: &mut Atmosphere,
        surf: &Surface,
        resize_diff: Option<(f32, f32)>,
    ) {
        log::debug!("xdg_surface: generating configure event {}", self.ss_serial);
        // send configuration requests to the client
        if let Some(toplevel) = &self.ss_xdg_toplevel {
            // Get the current window position
            let mut size;
            // if the client manually requested a size, honor that
            if let Some(cur_size) = self.ss_xs.xs_size {
                size = cur_size;
            } else {
                // If we don't have the size saved then grab the latest
                // from atmos
                let raw_size = atmos.get_window_size(surf.s_id);
                size = (raw_size.0 as i32, raw_size.1 as i32);

                if let Some((x, y)) = resize_diff {
                    // update our state's dimensions
                    // We SHOULD NOT update the atmosphere until the wl_surface
                    // is committed
                    size.0 += x as i32;
                    size.1 += y as i32;
                }
            }

            // build an array of state flags to pass to toplevel.configure
            let mut states: Vec<u8> = Vec::new();
            if self.ss_cur_tlstate.tl_maximized {
                states.push(xdg_toplevel::State::Maximized as u8);
            }
            if self.ss_cur_tlstate.tl_resizing {
                states.push(xdg_toplevel::State::Resizing as u8);
            }
            if self.ss_cur_tlstate.tl_fullscreen {
                states.push(xdg_toplevel::State::Fullscreen as u8);
            }
            log::debug!("xdg_surface: sending states {:?}", states);

            // insert a tlconfig representing this configure event.
            // commit will find the latest acked tlconfig we add
            // to this list and use its info
            let tlc_size = self.ss_tlconfigs.len();
            if tlc_size > 0 && *self.ss_tlconfigs[tlc_size - 1].tlc_state == self.ss_cur_tlstate {
                // If nothing has changed, clone the previous rc
                // instead of allocating
                self.ss_tlconfigs.push(TLConfig::new(
                    self.ss_serial,
                    size.0,
                    size.1, // width, height
                    self.ss_tlconfigs[tlc_size - 1].tlc_state.clone(),
                ));
            } else {
                self.ss_tlconfigs.push(TLConfig::new(
                    self.ss_serial,
                    size.0,
                    size.1, // width, height
                    Rc::new(self.ss_cur_tlstate),
                ));
            }
            log::debug!(
                "xdg_surface: pushing config {:?}",
                self.ss_tlconfigs[self.ss_tlconfigs.len() - 1]
            );

            // send them to the client
            toplevel.configure(size.0 as i32, size.1 as i32, states);
        }

        self.ss_xdg_surface.configure(self.ss_serial);
        self.ss_serial += 1;
    }

    /// Check if this serial is the currently loaned out one,
    /// and if so set the existing state to be applied
    pub fn ack_configure(&mut self, serial: u32) {
        // ack that we should take action during the next commit
        self.ss_xs.xs_acked = true;

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
    fn set_win_geom(&mut self, x: i32, y: i32, width: i32, height: i32) {
        let atmos_cell = self.ss_atmos.clone();
        let surf_cell = self.ss_surface.clone();
        let mut atmos = atmos_cell.borrow_mut();
        let mut surf = surf_cell.borrow_mut();

        // we need to update the *window* position
        // to be an offset from the base surface position
        let mut surf_pos = atmos.get_surface_pos(surf.s_id);
        surf_pos.0 += x as f32;
        surf_pos.1 += y as f32;
        atmos.set_window_pos(surf.s_id, surf_pos.0, surf_pos.0);

        self.ss_xs.xs_size = Some((width, height));
        // Go ahead and generate a configure event.
        // If this is called by the client, we need to trigger an event
        // manually TODO?
        self.configure(&mut atmos, &mut surf, None);
    }

    /// Get a toplevel surface
    ///
    /// A toplevel surface is the "normal" window type. It
    /// represents displaying one wl_surface in the desktop shell.
    /// `userdata` is a Rc ref of ourselves which should
    /// be added to toplevel.
    fn get_toplevel(
        &mut self,
        toplevel: Main<xdg_toplevel::XdgToplevel>,
        userdata: Rc<RefCell<ShellSurface>>,
    ) {
        // Mark our surface as being a window handled by xdg_shell
        self.ss_surface.borrow_mut().s_role = Some(Role::xdg_shell_toplevel(userdata.clone()));

        // Record our state
        self.ss_xs.xs_make_new_toplevel_window = true;

        // send configuration requests to the client
        // width and height 0 means client picks a size
        toplevel.configure(
            0,
            0,
            Vec::new(), // TODO: specify our requirements?
        );
        self.ss_xdg_surface.configure(self.ss_serial);
        self.ss_serial += 1;

        // Now add ourselves to the xdg_toplevel
        self.ss_xdg_toplevel = Some(toplevel.clone());
        toplevel.quick_assign(move |t, r, _| {
            userdata.borrow_mut().handle_toplevel_request(t, r);
        });
    }

    /// handle xdg_toplevel requests
    ///
    /// This should load our xdg state with any changes that the client
    /// has made, and they will be applied during the next commit.
    fn handle_toplevel_request(
        &mut self,
        _toplevel: Main<xdg_toplevel::XdgToplevel>,
        req: xdg_toplevel::Request,
    ) {
        let xs = &mut self.ss_xs;

        #[allow(unused_variables)]
        match req {
            xdg_toplevel::Request::Destroy => (),
            xdg_toplevel::Request::SetParent { parent } => (),
            xdg_toplevel::Request::SetTitle { title } => xs.xs_title = Some(title),
            xdg_toplevel::Request::SetAppId { app_id } => xs.xs_app_id = Some(app_id),
            xdg_toplevel::Request::ShowWindowMenu { seat, serial, x, y } => (),
            xdg_toplevel::Request::Move { seat, serial } => {
                // Moving is NOT double buffered so just grab it now
                let id = self.ss_surface.borrow().s_id;
                self.ss_atmos.borrow_mut().set_grabbed(Some(id));
            }
            xdg_toplevel::Request::Resize {
                seat,
                serial,
                edges,
            } => {
                // Moving is NOT double buffered so just grab it now
                let id = self.ss_surface.borrow().s_id;
                self.ss_atmos.borrow_mut().set_resizing(Some(id));
                self.ss_cur_tlstate.tl_resizing = true;
            }
            xdg_toplevel::Request::SetMaxSize { width, height } => {
                xs.xs_max_size = Some((width, height))
            }
            xdg_toplevel::Request::SetMinSize { width, height } => {
                xs.xs_min_size = Some((width, height))
            }
            xdg_toplevel::Request::SetMaximized => self.ss_cur_tlstate.tl_maximized = true,
            xdg_toplevel::Request::UnsetMaximized => self.ss_cur_tlstate.tl_maximized = false,
            xdg_toplevel::Request::SetFullscreen { output } => {
                self.ss_cur_tlstate.tl_fullscreen = true
            }
            xdg_toplevel::Request::UnsetFullscreen => self.ss_cur_tlstate.tl_fullscreen = false,
            xdg_toplevel::Request::SetMinimized => self.ss_cur_tlstate.tl_minimized = true,
        }
    }

    /// Calculate the position for this popup, and generate configure
    /// events broadcasting it.
    /// This will use the repositioned value if it was set.
    fn reposition_popup(&mut self) {
        let pop = self.ss_xdg_popup.as_mut().unwrap();
        if let Some(repo) = pop.pu_next_positioner.take() {
            pop.pu_positioner = repo;
        }

        let pos_cell = pop
            .pu_positioner
            .as_ref()
            .user_data()
            .get::<Rc<RefCell<Positioner>>>()
            .expect("Bug: positioner did not have userdata attached")
            .clone();
        let pos = pos_cell.borrow();

        // send configuration requests to the client
        // width and height 0 means client picks a size
        let popup_loc = pos.get_loc();
        log::error!("Popup location: {:?}", popup_loc);
        pop.pu_pop
            .configure(popup_loc.0, popup_loc.1, pos.p_width, pos.p_height);
        self.ss_xdg_surface.configure(self.ss_serial);
        self.ss_serial += 1;
    }

    /// Register a new popup surface.
    ///
    /// A popup surface is for dropdowns and alerts, and is the consumer
    /// of the positioner code. It is assigned a position over a parent
    /// toplevel surface and may exclusively grab input for that app.
    fn get_popup(
        &mut self,
        popup: Main<xdg_popup::XdgPopup>,
        parent: Option<xdg_surface::XdgSurface>,
        positioner: xdg_positioner::XdgPositioner,
        userdata: Rc<RefCell<ShellSurface>>,
    ) {
        // assign the popup role
        self.ss_surface.borrow_mut().s_role = Some(Role::xdg_shell_popup(userdata.clone()));

        // tell vkcomp to generate resources for a new window
        self.ss_xs.xs_make_new_popup_window = true;

        self.ss_xdg_popup = Some(Popup {
            pu_pop: popup.clone(),
            pu_parent: parent,
            pu_positioner: positioner,
            pu_next_positioner: None,
            pu_reposition: None,
        });

        popup.quick_assign(move |p, r, _| {
            userdata.borrow_mut().handle_popup_request(p, r);
        });
        self.reposition_popup();
    }

    fn popup_done(&mut self) {
        // getting the xdg_popup::Destroy event is making firefox destroy the dmabuf I think...
        self.ss_surface
            .borrow_mut()
            .destroy(&mut self.ss_atmos.borrow_mut());
        self.ss_xdg_popup.as_ref().unwrap().pu_pop.popup_done();
    }

    /// handle xdg_popup requests
    ///
    /// This should load our xdg state with any changes that the client
    /// has made, and they will be applied during the next commit.
    /// There is relatively little compared to xdg_toplevel.
    fn handle_popup_request(&mut self, _popup: Main<xdg_popup::XdgPopup>, req: xdg_popup::Request) {
        // TODO: implement the remaining handlers
        #[allow(unused_variables)]
        match req {
            xdg_popup::Request::Destroy => {
                log::debug!("Popup destroyed. Dismissing it");
                self.popup_done();
            }
            // TODO: implement grab
            xdg_popup::Request::Grab { seat, serial } => {
                log::error!("Grabbing a popup is not supported");
                self.popup_done();
            }
            xdg_popup::Request::Reposition { positioner, token } => {
                self.ss_xdg_popup.as_mut().unwrap().pu_next_positioner = Some(positioner);
            }
        }
    }
}
