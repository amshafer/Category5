// Implementation of the xdg_shell and xdg_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::wl_surface;

use crate::category5::vkcomp::wm;
use super::surface::*;
use super::protocol::xdg_shell::*;
use super::role::Role;

use std::rc::Rc;
use std::cell::RefCell;
use std::clone::Clone;

// This is the set of outstanding
// configuration changes which have not
// been committed yet.
#[allow(dead_code)]
struct XdgState {
    // window title
    xs_title: Option<String>,
    // Should we create a new window
    xs_make_toplevel: bool,
}

impl XdgState {
    // Return a state with no changes
    fn empty() -> XdgState {
        XdgState {
            xs_title: None,
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

            let shsurf = Rc::new(RefCell::new(ShellSurface {
                ss_surface: surf.clone(),
                ss_surface_proxy: surface,
                ss_attached_state: XdgState::empty(),
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
    // Category5 surface state object
    ss_surface: Rc<RefCell<Surface>>,
    // the wayland proxy
    ss_surface_proxy: wl_surface::WlSurface,
    // Outstanding changes to be applied in commit
    ss_attached_state: XdgState
}

impl ShellSurface {
    // Surface is the caller, and it has already called
    // borrow_mut, so it will just pass itself to us
    // to prevent causing a refcell panic.
    pub fn commit(&mut self, surf: &Surface) {
        let xs = &self.ss_attached_state;

        // This has just been assigned role of toplevel
        if xs.xs_make_toplevel {
            // Tell vkcomp to create a new window
            println!("Setting surface {} to toplevel", surf.s_id);
            surf.s_wm_tx.send(
                wm::task::Task::create_window(
                    surf.s_id, // ID of the new window
                    0, 0, // position
                    // No texture yet, it will be added by Surface
                    640, 480, // window dimensions
                )
            ).unwrap();
        }

        // Reset the state now that it is complete
        self.ss_attached_state = XdgState::empty();
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

        // Now add ourselves to the xdg_toplevel
        toplevel.quick_assign(|_,_,_| {});
        toplevel.as_ref().user_data().set(move || userdata);
    }
}
