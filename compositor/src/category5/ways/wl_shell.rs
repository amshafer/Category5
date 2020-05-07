// Implementation of the wl_shell and wl_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::{wl_shell,wl_shell_surface, wl_surface};

// Handle requests to a wl_shell interface
//
// The wl_shell interface implements functionality regarding
// the lifecycle of the window. Essentially it just creates
// a wl_shell_surface.
pub fn wl_shell_handle_request(req: wl_shell::Request,
                               _shell: Main<wl_shell::WlShell>)
{
    match req {
        wl_shell::Request::GetShellSurface { id: shell_surface, surface } => {
            let shsurf = ShellSurface {
                ss_surface: surface,
            };

            shell_surface.quick_assign(|s, r, _| {
                wl_shell_surface_handle_request(s, r);
            });
            // Pass ourselves as user data
            shell_surface.as_ref().user_data().set(move || shsurf);
        },
        _ => unimplemented!(),
    };
}

// Handle requests to a wl_shell_surface interface
//
// wl_shell_surface is the interface which actually
// tracks window characteristics and roles. It is
// highly recommended to read wayland.xml for all
// the gory details.
fn wl_shell_surface_handle_request(surf: Main<wl_shell_surface::WlShellSurface>,
                                   req: wl_shell_surface::Request)
{
    let _shsurf = surf.as_ref().user_data().get::<ShellSurface>().unwrap();

    match req {
        wl_shell_surface::Request::SetToplevel => {},
        _ => unimplemented!(),
    };
}

// A shell surface
//
// This is the private protocol object for
// wl_shell_surface. It actually implements
// all the functionality that the handler
// dispatches
struct ShellSurface {
    ss_surface: wl_surface::WlSurface,
}
