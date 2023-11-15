// Implementation of the wl_shell and wl_shell_surface
// interfaces
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::protocol::{wl_shell, wl_shell_surface, wl_surface};
use ws::Resource;

use super::role::Role;
use super::surface::*;
use crate::category5::atmosphere::Atmosphere;
use crate::category5::vkcomp::wm;
use crate::category5::Climate;

use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

#[allow(unused_variables)]
impl ws::GlobalDispatch<wl_shell::WlShell, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<wl_shell::WlShell>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wl_shell::WlShell, ()> for Climate {
    // Handle requests to a wl_shell interface
    //
    // The wl_shell interface implements functionality regarding
    // the lifecycle of the window. Essentially it just creates
    // a wl_shell_surface.
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_shell::WlShell,
        request: wl_shell::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        match request {
            wl_shell::Request::GetShellSurface { id, surface } => {
                // get category5's surface from the userdata
                let surf = surface.data::<Arc<Mutex<Surface>>>().unwrap();

                let shsurf = Arc::new(Mutex::new(ShellSurface {
                    ss_surface: surf.clone(),
                    ss_surface_proxy: surface,
                    ss_toplevel: false,
                }));

                // Pass ourselves as user data
                data_init.init(id, shsurf);
            }
            _ => unimplemented!(),
        };
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &(),
    ) {
    }
}

#[allow(unused_variables)]
impl ws::Dispatch<wl_shell_surface::WlShellSurface, Arc<Mutex<ShellSurface>>> for Climate {
    // Handle requests to a wl_shell_surface interface
    //
    // wl_shell_surface is the interface which actually
    // tracks window characteristics and roles. It is
    // highly recommended to read wayland.xml for all
    // the gory details.
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_shell_surface::WlShellSurface,
        request: wl_shell_surface::Request,
        data: &Arc<Mutex<ShellSurface>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        let mut shsurf = data.lock().unwrap();

        match request {
            wl_shell_surface::Request::SetToplevel => {
                shsurf.set_toplevel(state.c_atmos.lock().unwrap().deref_mut())
            }
            wl_shell_surface::Request::SetTitle { title } => {}
            _ => unimplemented!(),
        };
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &Arc<Mutex<ShellSurface>>,
    ) {
    }
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
    ss_surface: Arc<Mutex<Surface>>,
    // the wayland proxy
    ss_surface_proxy: wl_surface::WlSurface,
    ss_toplevel: bool,
}

impl ShellSurface {
    fn set_toplevel(&mut self, atmos: &mut Atmosphere) {
        self.ss_toplevel = true;

        // Tell vkcomp to create a new window
        let mut surf = self.ss_surface.lock().unwrap();
        println!("Setting surface {:?} to toplevel", surf.s_id.get_raw_id());

        atmos.a_toplevel.set(&surf.s_id, true);
        atmos.add_wm_task(wm::task::Task::new_toplevel(surf.s_id.clone()));
        // This places the surface at the front of the skiplist, aka
        // makes it in focus
        atmos.focus_on(Some(surf.s_id.clone()));

        // Mark our surface as being a window handled by wl_shell
        surf.s_role = Some(Role::wl_shell_toplevel);
    }
}
