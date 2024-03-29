// Implementation of the wl_pointer interface
//
// Austin Shafer - 2020
extern crate utils;
extern crate wayland_server as ws;
use super::role::Role;
use super::surface::Surface;
use crate::category5::Climate;
use std::sync::{Arc, Mutex};
use utils::log;
use ws::protocol::wl_pointer;
use ws::{Resource, ResourceData};

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wl_pointer::WlPointer, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_pointer::WlPointer,
        request: wl_pointer::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        match request {
            wl_pointer::Request::SetCursor {
                surface,
                hotspot_x,
                hotspot_y,
                ..
            } => {
                let id = if let Some(surface) = surface {
                    let data = surface
                            .object_data()
                            .unwrap()
                            .clone()
                            .downcast::<ResourceData<ws::protocol::wl_surface::WlSurface, Arc<Mutex<Surface>>>>()
                            .unwrap();
                    let mut surf = data.udata.lock().unwrap();

                    // It is a protocol error to have another role assigned at this time
                    if match surf.s_role {
                        None | Some(Role::cursor) => false,
                        _ => true,
                    } {
                        resource.post_error(
                            wl_pointer::Error::Role,
                            "Surface is already assigned a non-cursor role",
                        );
                    }

                    surf.s_role = Some(Role::cursor);

                    Some(surf.s_id.clone())
                } else {
                    None
                };

                let mut atmos = state.c_atmos.lock().unwrap();

                atmos.set_cursor(id);
                log::debug!("Setting cursor hotspot to {:?}", (hotspot_x, hotspot_y));
                atmos.set_cursor_hotspot((hotspot_x, hotspot_y));
            }
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &wl_pointer::WlPointer,
        data: &(),
    ) {
    }
}
