// Implementation of the xdg_shell and xdg_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::wl_surface;

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

// This is the set of outstanding
// configuration changes which have not
// been committed yet.
//
// Configuration is a bit weird, I think it looks like:
// first: any role interfaces will send their own configure events, which
//   request the client to set itself to match a certain state (size, maximized, etc)
// second: once that is done, the xdg_wm_base will send a configure event
//   saying that the configuration requests are over with.
// thirdish: the client will start making requests that update each part
//   of the window state (i.e. set the size/title)
// fourth: the client will do the ack_configure request to tell the server
//   that it is done.
// finally: the client will commit the surface, causing the server to apply
//   all of the attached state
#[allow(dead_code)]
#[derive(Clone)]
pub struct XdgState {
    pub xs_acked: bool,
    // window title
    pub xs_title: Option<String>,
    // self-explanitory I think
    pub xs_maximized: bool,
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

    // ------------------
    // The following are "meta" configuration changes
    // aka making new role objects, not related to the
    // window itself
    // ------------------
    // Should we create a new window
    xs_make_toplevel: bool,
}

impl XdgState {
    // Return a state with no changes
    fn empty() -> XdgState {
        XdgState {
            xs_acked: false,
            xs_title: None,
            xs_maximized: false,
            xs_fullscreen: false,
            xs_resizing: false,
            xs_activated: false,
            xs_tiled_left: false,
            xs_tiled_right: false,
            xs_tiled_top: false,
            xs_tiled_bottom: false,
            xs_make_toplevel: false,
        }
    }
}

// Handle requests to a xdg_shell interface
//
// The xdg_shell interface implements functionality regarding
// the lifecycle of the window. Essentially it just creates
// a xdg_shell_surface.
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
                p_anchor_rect: None,
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

// Handle requests to a xdg_shell_surface interface
//
// xdg_shell_surface is the interface which actually
// tracks window characteristics and roles. It is
// highly recommended to read wayland.xml for all
// the gory details.
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
        xdg_surface::Request::AckConfigure { serial } =>
            shsurf.ack_configure(serial),
        _ => unimplemented!(),
    };
}

// A positioner object
//
// This is used to position popups relative to the toplevel parent
// surface. It handles offsets and anchors for hovering the popup
// surface.
#[derive(Copy,Clone)]
struct Positioner {
    /// The dimensions of the positioned surface
    p_x: i32,
    p_y: i32,
    p_width: i32,
    p_height: i32,
    /// (x, y, width, height) of the anchor rectangle
    p_anchor_rect: Option<(i32, i32, i32, i32)>,
    p_anchor: xdg_positioner::Anchor,
    p_gravity: xdg_positioner::Gravity,
    p_constraint: xdg_positioner::ConstraintAdjustment,
    /// If the constraints should be recalculated when the parent is moved
    p_reactive: bool,
    p_parent_size: Option<(i32, i32)>,
    /// The serial of the parent configuration event this is responding to
    p_parent_configure: u32,
}

fn xdg_positioner_handle_request(res: Main<xdg_positioner::XdgPositioner>,
                                 req: xdg_positioner::Request)
{
    let mut pos = *res.as_ref()
        .user_data()
        .get::<Positioner>()
        .expect("xdg_positioner did not contain the correct userdata");

    match req {
        xdg_positioner::Request::SetSize { width, height } => {
            pos.p_width = width;
            pos.p_height = height;
        },
        xdg_positioner::Request::SetAnchorRect { x, y, width, height } => {
            pos.p_anchor_rect = Some((x, y, width, height));
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

// A shell surface
//
// This is the private protocol object for
// xdg_shell_surface. It actually implements
// all the functionality that the handler
// dispatches
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
    // Outstanding changes to be applied in commit
    pub ss_attached_state: XdgState,
    pub ss_current_state: XdgState,
    // A serial for tracking config updates
    // every time we request a configuration this is the
    // serial number used
    pub ss_serial: u32,
}

impl ShellSurface {
    // Surface is the caller, and it has already called
    // borrow_mut, so it will just pass itself to us
    // to prevent causing a refcell panic.
    // The same goes for the atmosphere
    pub fn commit(&mut self, surf: &Surface, atmos: &mut Atmosphere) {
        let xs = &mut self.ss_attached_state;
        // do nothing if the client has yet to ack these changes
        if !xs.xs_acked {
            return;
        }

        // This has just been assigned role of toplevel
        if xs.xs_make_toplevel {
            // Tell vkcomp to create a new window
            println!("Setting surface {} to toplevel", surf.s_id);
            atmos.create_new_window(surf.s_id);
            xs.xs_make_toplevel = false;
        }
        self.ss_current_state = self.ss_attached_state.clone();
    }

    // Generate a fresh set of configure events
    //
    // This is called from other subsystems (input), which means we need to
    // pass the surface as an argument since its refcell will already be
    // borrowed.
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

    // Check if this serial is the currently loaned out one,
    // and if so set the existing state to be applied
    pub fn ack_configure(&mut self, serial: u32) {
        if serial == self.ss_serial {
            // mark this as acked so it is applied in commit
            self.ss_attached_state.xs_acked = true;
            // increment the serial for next timme
            self.ss_serial += 1;
        }
    }

    // userdata is a Rc ref of ourselves which should
    // be added to toplevel
    fn get_toplevel(&mut self,
                    toplevel: Main<xdg_toplevel::XdgToplevel>,
                    userdata: Rc<RefCell<ShellSurface>>)
    {
        // Mark our surface as being a window handled by xdg_shell
        self.ss_surface.borrow_mut().s_role =
            Some(Role::xdg_shell_toplevel(userdata.clone()));

        // Record our state
        self.ss_attached_state.xs_make_toplevel = true;

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
        toplevel.quick_assign(|_,_,_| {});
        // TODO: implement handler for stuff like resize
        toplevel.as_ref().user_data().set(move || userdata);
    }
}
