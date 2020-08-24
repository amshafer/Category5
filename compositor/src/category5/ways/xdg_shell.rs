// Implementation of the xdg_shell and xdg_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::wl_surface;

use super::surface::*;
use super::role::Role;
use super::protocol::xdg_shell::*;
use crate::category5::utils::atmosphere::Atmosphere;

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
struct XdgState {
    xs_acked: bool,
    // window title
    xs_title: Option<String>,
    // the width and height of the window
    xs_width: i32,
    xs_height: i32,
    // self-explanitory I think
    xs_maximized: bool,
    // guess what this one means
    xs_fullscreen: bool,
    // who would have thought
    xs_resizing: bool,
    // Is the window currently in focus?
    xs_activated: bool,
    // Is this window against a tile boundary?
    xs_tiled_left: bool,
    xs_tiled_right: bool,
    xs_tiled_top: bool,
    xs_tiled_bottom: bool,

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
            xs_width: 0,
            xs_height: 0,
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
                ss_atmos: atmos,
                ss_surface: surf.clone(),
                ss_surface_proxy: surface,
                ss_xdg_surface: xdg.clone(),
                ss_attached_state: XdgState::empty(),
                ss_serial: 0,
            }));

            xdg.quick_assign(|s, r, _| {
                xdg_surface_handle_request(s, r);
            });
            // Pass ourselves as user data
            xdg.as_ref().user_data().set(move || shsurf);
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
    // Outstanding changes to be applied in commit
    ss_attached_state: XdgState,
    // A serial for tracking config updates
    // every time we request a configuration this is the
    // serial number used
    ss_serial: u32,
}

impl ShellSurface {
    // Surface is the caller, and it has already called
    // borrow_mut, so it will just pass itself to us
    // to prevent causing a refcell panic.
    pub fn commit(&mut self, surf: &Surface) {
        let xs = &self.ss_attached_state;
        // do nothing if the client has yet to ack these changes
        if !xs.xs_acked {
            return;
        }

        // This has just been assigned role of toplevel
        if xs.xs_make_toplevel {
            // Tell vkcomp to create a new window
            println!("Setting surface {} to toplevel", surf.s_id);
            surf.s_atmos.borrow_mut().create_new_window(surf.s_id);
        }
        // Reset the state now that it is complete
        self.ss_attached_state = XdgState::empty();
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
        let mut surf = self.ss_surface.borrow_mut();        

        // Mark our surface as being a window handled by xdg_shell
        surf.s_role = Some(Role::xdg_shell_toplevel(userdata.clone()));

        // Record our state
        self.ss_attached_state.xs_make_toplevel = true;

        // send configuration requests to the client
        toplevel.configure(
            // width and height 0 means client picks a size
            0, 0,
            Vec::new(), // TODO: specify our requirements?
        );
        self.ss_xdg_surface.configure(self.ss_serial);

        // Now add ourselves to the xdg_toplevel
        // TODO: implement toplevel
        toplevel.quick_assign(|_,_,_| {});
        toplevel.as_ref().user_data().set(move || userdata);
    }
}
