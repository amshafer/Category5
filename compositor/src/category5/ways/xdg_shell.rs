// Implementation of the xdg_shell and xdg_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::wl_surface;

use super::utils;
use super::surface::*;
use super::role::Role;
pub use super::protocol::xdg_shell::*;

use crate::category5::utils::{
    timing::*, logging::LogLevel, atmosphere::Atmosphere,
};
use crate::log;

use std::rc::Rc;
use std::cell::RefCell;
use std::clone::Clone;

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
#[derive(Clone)]
pub struct XdgState {
    pub xs_width: i32,
    pub xs_height: i32,
    // window title
    pub xs_title: Option<String>,
    pub xs_app_id: Option<String>,
    // self-explanitory I think
    pub xs_maximized: bool,
    pub xs_minimized: bool,
    // guess what this one means
    pub xs_fullscreen: bool,
    // who would have thought
    pub xs_resizing: bool,
    // Is the window currently in focus?
    pub xs_activated: bool,
    // Is this window against a tile boundary?
    pub xs_tiled_left: bool,
    pub xs_tiled_right: bool,
    pub xs_tiled_top: bool,
    pub xs_tiled_bottom: bool,
    // bounding dimensions
    // (0, 0) means it has not yet been set
    pub xs_max_size: (i32, i32),
    pub xs_min_size: (i32, i32),

    // ------------------
    // The following are "meta" configuration changes
    // aka making new role objects, not related to the
    // window itself
    // ------------------
    // Should we create a new window
    xs_make_new_window: bool,
    pub xs_acked: bool,
}

impl XdgState {
    /// Return a state with no changes
    fn empty() -> XdgState {
        XdgState {
            xs_width: 0,
            xs_height: 0,
            xs_acked: false,
            xs_title: None,
            xs_app_id: None,
            xs_maximized: false,
            xs_fullscreen: false,
            xs_minimized: false,
            xs_resizing: false,
            xs_activated: false,
            xs_tiled_left: false,
            xs_tiled_right: false,
            xs_tiled_top: false,
            xs_tiled_bottom: false,
            xs_max_size: (0, 0),
            xs_min_size: (0, 0),
            xs_make_new_window: false,
        }
    }
}

/// Handle requests to a xdg_shell interface
///
/// The xdg_shell interface implements functionality regarding
/// the lifecycle of the window. Essentially it just creates
/// a xdg_shell_surface.
pub fn xdg_wm_base_handle_request(req: xdg_wm_base::Request,
                                  _wm_base: Main<xdg_wm_base::XdgWmBase>)
{
    match req {
        xdg_wm_base::Request::GetXdgSurface { id: xdg, surface } => {
            // get category5's surface from the userdata
            let surf = surface.as_ref()
                .user_data()
                .get::<Rc<RefCell<Surface>>>()
                .unwrap();
            let atmos = surf.borrow_mut().s_atmos.clone();

            let shsurf = Rc::new(RefCell::new(ShellSurface {
                ss_atmos: atmos.clone(),
                ss_surface: surf.clone(),
                ss_surface_proxy: surface,
                ss_xdg_surface: xdg.clone(),
                ss_attached_state: XdgState::empty(),
                ss_current_state: XdgState::empty(),
                ss_serial: 0,
                ss_xdg_toplevel: None,
                ss_xdg_popup: None,
            }));

            xdg.quick_assign(|s, r, _| {
                xdg_surface_handle_request(s, r);
            });
            // Pass ourselves as user data
            xdg.as_ref().user_data().set(move || shsurf);
        },
        xdg_wm_base::Request::CreatePositioner { id } => {
            let pos = Positioner {
                p_x: 0, p_y: 0, p_width: 0, p_height: 0,
                p_anchor_rect: (0, 0, 0, 0),
                p_anchor: xdg_positioner::Anchor::None,
                p_gravity: xdg_positioner::Gravity::None,
                p_constraint: xdg_positioner::ConstraintAdjustment::None,
                p_reactive: false,
                p_parent_size: None,
                p_parent_configure: 0,
            };

            id.quick_assign(|s, r, _| {
                xdg_positioner_handle_request(s, r);
            });
            // We will add the positioner as userdata since nothing
            // else needs to look it up
            id.as_ref().user_data().set(move || pos);
        },
        _ => unimplemented!(),
    };
}

/// Handle requests to a xdg_shell_surface interface
///
/// xdg_shell_surface is the interface which actually
/// tracks window characteristics and roles. It is
/// highly recommended to read wayland.xml for all
/// the gory details.
fn xdg_surface_handle_request(surf: Main<xdg_surface::XdgSurface>,
                              req: xdg_surface::Request)
{
    // first clone the ShellSurface to be used as
    // userdata later
    let ss_clone = surf.as_ref()
        .user_data()
        .get::<Rc<RefCell<ShellSurface>>>()
        .unwrap()
        .clone();

    // Now get a ref to the ShellSurface
    let mut shsurf = surf.as_ref()
        .user_data()
        .get::<Rc<RefCell<ShellSurface>>>()
        .unwrap()
        .borrow_mut();

    match req {
        xdg_surface::Request::GetToplevel { id: xdg } =>
            shsurf.get_toplevel(xdg, ss_clone),
        xdg_surface::Request::GetPopup { id, parent, positioner } =>
            shsurf.get_popup(id, parent, positioner, ss_clone),
        xdg_surface::Request::AckConfigure { serial } =>
            shsurf.ack_configure(serial),
        xdg_surface::Request::SetWindowGeometry { x, y, width, height } =>
            shsurf.set_win_geom(x, y, width, height),
        _ => unimplemented!(),
    };
}

/// A positioner object
///
/// This is used to position popups relative to the toplevel parent
/// surface. It handles offsets and anchors for hovering the popup
/// surface.
#[derive(Copy,Clone)]
struct Positioner {
    /// The dimensions of the positioned surface
    p_x: i32, // from set_offset
    p_y: i32,
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
        // TODO: add the rest of the positioner elements
        (self.p_anchor_rect.0 + self.p_x,
         self.p_anchor_rect.1 + self.p_y)
    }
}

/// Respond to xdg_positioner requests.
///
/// These requests are used to build up a `Positioner`, which will
/// later be used during the creation of an `xdg_popup` surface.
fn xdg_positioner_handle_request(res: Main<xdg_positioner::XdgPositioner>,
                                 req: xdg_positioner::Request)
{
    let mut pos = *res.as_ref()
        .user_data()
        .get::<Positioner>()
        .expect("xdg_positioner did not contain the correct userdata");

    // add the reqeust data to our struct
    match req {
        xdg_positioner::Request::SetSize { width, height } => {
            pos.p_width = width;
            pos.p_height = height;
        },
        xdg_positioner::Request::SetAnchorRect { x, y, width, height } => {
            pos.p_anchor_rect = (x, y, width, height);
        },
        xdg_positioner::Request::SetAnchor { anchor } =>
            pos.p_anchor = anchor,
        xdg_positioner::Request::SetGravity { gravity } =>
            pos.p_gravity = gravity,
        xdg_positioner::Request::SetConstraintAdjustment { constraint_adjustment } =>
            pos.p_constraint = xdg_positioner::ConstraintAdjustment::from_raw(constraint_adjustment)
            .unwrap(),
        xdg_positioner::Request::SetOffset { x, y } => {
            pos.p_x = x;
            pos.p_y = y;
        },
        xdg_positioner::Request::SetReactive =>
            pos.p_reactive = true,
        xdg_positioner::Request::SetParentSize { parent_width, parent_height } =>
            pos.p_parent_size = Some((parent_width, parent_height)),
        xdg_positioner::Request::SetParentConfigure { serial } =>
            pos.p_parent_configure = serial,
        _ => unimplemented!(),
    };

    // store the updated Positioner in the userdata
    res.as_ref().user_data().set(move || pos);
}

/// Private struct for an xdg_popup role.
#[allow(dead_code)]
pub struct Popup {
    pu_pop: Main<xdg_popup::XdgPopup>,
    pu_parent: Option<xdg_surface::XdgSurface>,
    pu_positioner: xdg_positioner::XdgPositioner,
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
    pub ss_attached_state: XdgState,
    pub ss_current_state: XdgState,
    // A serial for tracking config updates
    // every time we request a configuration this is the
    // serial number used
    pub ss_serial: u32,
}

impl ShellSurface {
    /// Surface is the caller, and it has already called
    /// borrow_mut, so it will just pass itself to us
    /// to prevent causing a refcell panic.
    /// The same goes for the atmosphere
    pub fn commit(&mut self, surf: &Surface, atmos: &mut Atmosphere) {
        let xs = &mut self.ss_attached_state;
        // do nothing if the client has yet to ack these changes
        if !xs.xs_acked {
            return;
        }

        // This has just been assigned role of toplevel
        if xs.xs_make_new_window {
            // Tell vkcomp to create a new window
            println!("Setting surface {} to toplevel", surf.s_id);
            let is_toplevel = match self.ss_xdg_toplevel {
                Some(_) => true,
                None => false,
            };
            let client = self.ss_xdg_surface.as_ref().client().unwrap();
            let owner = utils::try_get_id_from_client(client).unwrap();
            atmos.create_new_window(surf.s_id, owner, is_toplevel);
            xs.xs_make_new_window = false;
        }

        // Update the window size
        let mut pos = atmos.get_window_dimensions(surf.s_id);
        pos.0 += xs.xs_width as f32;
        pos.1 += xs.xs_height as f32;
        atmos.set_window_dimensions(surf.s_id,
                                    pos.0, pos.1,
                                    pos.2, pos.3);

        // TODO: handle the other state changes
        //       make them options??

        // unset the ack for next time
        xs.xs_acked = false;
        self.ss_current_state = self.ss_attached_state.clone();
    }

    /// Generate a fresh set of configure events
    ///
    /// This is called from other subsystems (input), which means we need to
    /// pass the surface as an argument since its refcell will already be
    /// borrowed.
    pub fn configure(&mut self,
                     atmos: &mut Atmosphere,
                     surf: &Surface,
                     resize_diff: Option<(f32,f32)>)
    {
        // send configuration requests to the client
        if let Some(toplevel) = &self.ss_xdg_toplevel {
            if let Some((x, y)) = resize_diff {
                // Get the current window position
                let mut dims = atmos.get_window_dimensions(surf.s_id);

                // update our state's dimensions
                dims.2 += x;
                dims.3 += y;
                log!(LogLevel::debug, "new window size is {}x{}",
                     dims.2,
                     dims.3,
                );
                // Update the atmosphere
                atmos.set_window_dimensions(surf.s_id,
                                            dims.0, dims.1, dims.2, dims.3);

                // send them to the client
                toplevel.configure(
                    dims.2 as i32, dims.3 as i32,
                    Vec::new(), // TODO: specify our requirements?
                );
            }
        }
        self.ss_xdg_surface.configure(self.ss_serial);
    }

    /// Check if this serial is the currently loaned out one,
    /// and if so set the existing state to be applied
    pub fn ack_configure(&mut self, serial: u32) {
        if serial == self.ss_serial {
            // mark this as acked so it is applied in commit
            self.ss_attached_state.xs_acked = true;
            // increment the serial for next timme
            self.ss_serial += 1;
        }
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
    fn set_win_geom(&mut self, _x: i32, _y: i32, width: i32, height: i32) {
        self.ss_attached_state.xs_width = width;
        self.ss_attached_state.xs_height = height;
    }

    /// Get a toplevel surface
    ///
    /// A toplevel surface is the "normal" window type. It
    /// represents displaying one wl_surface in the desktop shell.
    /// `userdata` is a Rc ref of ourselves which should
    /// be added to toplevel.
    fn get_toplevel(&mut self,
                    toplevel: Main<xdg_toplevel::XdgToplevel>,
                    userdata: Rc<RefCell<ShellSurface>>)
    {
        // Mark our surface as being a window handled by xdg_shell
        self.ss_surface.borrow_mut().s_role =
            Some(Role::xdg_shell_toplevel(userdata.clone()));

        // Record our state
        self.ss_attached_state.xs_make_new_window = true;

        // send configuration requests to the client
        // width and height 0 means client picks a size
        toplevel.configure(
            0, 0,
            Vec::new(), // TODO: specify our requirements?
        );
        self.ss_xdg_surface.configure(self.ss_serial);

        // Now add ourselves to the xdg_toplevel
        // TODO: implement toplevel
        self.ss_xdg_toplevel = Some(toplevel.clone());
        toplevel.quick_assign(move |t,r,_| {
            userdata.borrow_mut().handle_toplevel_request(t, r);
        });
    }

    /// handle xdg_toplevel requests
    ///
    /// This should load our xdg state with any changes that the client
    /// has made, and they will be applied during the next commit.
    fn handle_toplevel_request(&mut self,
                               _toplevel: Main<xdg_toplevel::XdgToplevel>,
                               req: xdg_toplevel::Request)
    {
        let xs = &mut self.ss_attached_state;

        // TODO: implement the remaining handlers
        #[allow(unused_variables)]
        match req {
            xdg_toplevel::Request::Destroy => (),
            xdg_toplevel::Request::SetParent { parent } => (),
            xdg_toplevel::Request::SetTitle { title } =>
                xs.xs_title = Some(title),
            xdg_toplevel::Request::SetAppId { app_id } =>
                xs.xs_app_id = Some(app_id),
            xdg_toplevel::Request::ShowWindowMenu { seat, serial, x, y } => (),
            xdg_toplevel::Request::Move { seat, serial } => (),
            xdg_toplevel::Request::Resize { seat, serial, edges } => (),
            xdg_toplevel::Request::SetMaxSize { width ,height } =>
                xs.xs_max_size = (width, height),
            xdg_toplevel::Request::SetMinSize { width, height } =>
                xs.xs_min_size = (width, height),
            xdg_toplevel::Request::SetMaximized =>
                xs.xs_maximized = true,
            xdg_toplevel::Request::UnsetMaximized =>
                xs.xs_maximized = false,
            xdg_toplevel::Request::SetFullscreen { output } =>
                xs.xs_fullscreen = true,
            xdg_toplevel::Request::UnsetFullscreen =>
                xs.xs_fullscreen = false,
            xdg_toplevel::Request::SetMinimized =>
                xs.xs_minimized = true,
        }
    }

    /// Register a new popup surface.
    ///
    /// A popup surface is for dropdowns and alerts, and is the consumer
    /// of the positioner code. It is assigned a position over a parent
    /// toplevel surface and may exclusively grab input for that app.
    fn get_popup(&mut self,
                 popup: Main<xdg_popup::XdgPopup>,
                 parent: Option<xdg_surface::XdgSurface>,
                 positioner: xdg_positioner::XdgPositioner,
                 userdata: Rc<RefCell<ShellSurface>>)
    {
        // assign the popup role
        self.ss_surface.borrow_mut().s_role =
            Some(Role::xdg_shell_popup(userdata.clone()));

        // tell vkcomp to generate resources for a new window
        self.ss_attached_state.xs_make_new_window = true;

        // send configuration requests to the client
        // width and height 0 means client picks a size
        // TODO: calculate the position according to the positioner rule
        let pos = positioner.as_ref()
            .user_data()
            .get::<Positioner>()
            .unwrap();
        let popup_loc = pos.get_loc();
        popup.configure(
            popup_loc.0, popup_loc.1, 0, 0, // x, y, width, height
        );
        self.ss_xdg_surface.configure(self.ss_serial);

        self.ss_xdg_popup = Some(Popup {
            pu_pop: popup.clone(),
            pu_parent: parent,
            pu_positioner: positioner,
        });

        popup.quick_assign(move |p,r,_| {
            userdata.borrow_mut().handle_popup_request(p, r);
        });
    }

    /// handle xdg_popup requests
    ///
    /// This should load our xdg state with any changes that the client
    /// has made, and they will be applied during the next commit.
    /// There is relatively little compared to xdg_toplevel.
    fn handle_popup_request(&mut self,
                            _popup: Main<xdg_popup::XdgPopup>,
                            req: xdg_popup::Request)
    {
        let _xs = &mut self.ss_attached_state;

        // TODO: implement the remaining handlers
        #[allow(unused_variables)]
        match req {
            xdg_popup::Request::Destroy => (),
            xdg_popup::Request::Grab { seat, serial } => (),
            xdg_popup::Request::Reposition { positioner, token } => (),
        }
    }
}
