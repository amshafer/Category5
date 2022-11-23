// Implementation of the wl_shell and wl_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::protocol::{wl_shell, wl_shell_surface, wl_surface};
use ws::Main;

use super::role::Role;
use super::surface::*;

use std::cell::RefCell;
use std::rc::Rc;

// Handle requests to a wl_shell interface
//
// The wl_shell interface implements functionality regarding
// the lifecycle of the window. Essentially it just creates
// a wl_shell_surface.
pub fn wl_shell_handle_request(req: wl_shell::Request, _shell: Main<wl_shell::WlShell>) {
    match req {
        wl_shell::Request::GetShellSurface {
            id: shell_surface,
            surface,
        } => {
            // get category5's surface from the userdata
            let surf = surface
                .as_ref()
                .user_data()
                .get::<Rc<RefCell<Surface>>>()
                .unwrap();

            let shsurf = Rc::new(RefCell::new(ShellSurface {
                ss_surface: surf.clone(),
                ss_surface_proxy: surface,
                ss_toplevel: false,
            }));

            shell_surface.quick_assign(|s, r, _| {
                wl_shell_surface_handle_request(s, r);
            });
            // Pass ourselves as user data
            shell_surface.as_ref().user_data().set(move || shsurf);
        }
        _ => unimplemented!(),
    };
}

// Handle requests to a wl_shell_surface interface
//
// wl_shell_surface is the interface which actually
// tracks window characteristics and roles. It is
// highly recommended to read wayland.xml for all
// the gory details.
#[allow(unused_variables)]
fn wl_shell_surface_handle_request(
    surf: Main<wl_shell_surface::WlShellSurface>,
    req: wl_shell_surface::Request,
) {
    let mut shsurf = surf
        .as_ref()
        .user_data()
        .get::<Rc<RefCell<ShellSurface>>>()
        .unwrap()
        .borrow_mut();

    match req {
        wl_shell_surface::Request::SetToplevel => shsurf.set_toplevel(),
        wl_shell_surface::Request::SetTitle { title } => {}
        _ => unimplemented!(),
    };
}

// A shell surface
//
// This is the private protocol object for
// wl_shell_surface. It actually implements
// all the functionality that the handler
// dispatches
#[allow(dead_code)]
pub struct ShellSurface {
    // Category5 surface state object
    ss_surface: Rc<RefCell<Surface>>,
    // the wayland proxy
    ss_surface_proxy: wl_surface::WlSurface,
    ss_toplevel: bool,
}

impl ShellSurface {
    fn set_toplevel(&mut self) {
        self.ss_toplevel = true;

        // Tell vkcomp to create a new window
        let mut surf = self.ss_surface.borrow_mut();
        println!("Setting surface {:?} to toplevel", surf.s_id);

        {
            let mut atmos = surf.s_atmos.lock().unwrap();
            atmos.set_toplevel(surf.s_id, true);
            // This places the surface at the front of the skiplist, aka
            // makes it in focus
            atmos.focus_on(Some(surf.s_id));
        }

        // Mark our surface as being a window handled by wl_shell
        surf.s_role = Some(Role::wl_shell_toplevel);
    }
}
