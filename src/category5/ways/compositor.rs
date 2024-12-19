// Wayland compositor singleton
//
// This is the "top" of the wayland heirarchy,
// it is the initiating module of the wayland
// protocols
//
// Austin Shafer - 2019
pub extern crate wayland_server as ws;

use super::surface::Surface;
use super::{utils, wl_region};
use crate::category5::Climate;

use ws::protocol::{wl_compositor as wlci, wl_surface as wlsi};

extern crate utils as cat5_utils;
use cat5_utils::log;

use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

#[allow(unused_variables)]
impl ws::GlobalDispatch<wlci::WlCompositor, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<wlci::WlCompositor>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

#[allow(unused_variables)]
impl ws::Dispatch<wlci::WlCompositor, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wlci::WlCompositor,
        request: ws::protocol::wl_compositor::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        match request {
            ws::protocol::wl_compositor::Request::CreateSurface { id } => {
                state.create_surface(client, id, data_init)
            }
            ws::protocol::wl_compositor::Request::CreateRegion { id } => {
                wl_region::register_new(id, data_init)
            }
            // All other requests are invalid
            _ => unimplemented!(),
        }
    }

    fn destroyed(
        _state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &wlci::WlCompositor,
        _data: &(),
    ) {
    }
}

impl Climate {
    /// wl_compositor interface create surface
    ///
    /// This request creates a new wl_surface and
    /// hooks up our surface handler. See the surface
    /// module
    pub fn create_surface(
        &mut self,
        client: &ws::Client,
        id: ws::New<wlsi::WlSurface>,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        let mut atmos = self.c_atmos.lock().unwrap();
        let client_id = utils::get_id_from_client(atmos.deref_mut(), client.clone());
        let win_id = atmos.mint_window_id(&mut self.c_scene, &client_id);
        log::debug!("Creating new surface {:?}", win_id.get_raw_id());

        // Create a reference counted object
        // in charge of this new surface
        let new_surface = Arc::new(Mutex::new(Surface::new(win_id.clone())));
        // Add the new surface to the atmosphere
        atmos.add_surface(&win_id, new_surface.clone());

        // Add the new surface to the userdata so other
        // protocols can see it
        let surf = data_init.init(id, new_surface);
        atmos.a_wl_surface.set(&win_id, surf.into());
    }
}
