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
use ws::Resource;

extern crate utils as cat5_utils;
use cat5_utils::log;

use std::cell::RefCell;
use std::rc::Rc;

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
                let surf = data_init.init(id, ());
                state.create_surface(surf, data_init)
            }
            ws::protocol::wl_compositor::Request::CreateRegion { id } => {
                wl_region::register_new(id)
            }
            // All other requests are invalid
            _ => unimplemented!(),
        }
    }

    fn destroyed(
        _state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
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
        surf: wlsi::WlSurface,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        let client = utils::get_id_from_client(
            &self.c_atmos,
            surf.client()
                .expect("client for this surface seems to have disappeared"),
        );
        let id = self.c_atmos.mint_window_id(client);
        log::debug!("Creating new surface {:?}", id);

        // Create a reference counted object
        // in charge of this new surface
        let new_surface = Surface::new(surf.clone(), id);
        // Add the new surface to the atmosphere
        self.c_atmos.add_surface(id, new_surface.clone());
        // get the Resource<WlSurface>, turn it into a &WlSurface
        self.c_atmos
            .set_wl_surface(id, surf.as_ref().clone().into());

        // Add the new surface to the userdata so other
        // protocols can see it
        data_init.init(surf, new_surface);
    }
}
