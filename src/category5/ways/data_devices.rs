// Implementations of inter-app data transfer operations. aka copy/paste and drag/drop
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::protocol::{
    wl_data_device as wlddv, wl_data_device_manager as wlddm, wl_data_source as wlds,
};

use crate::category5::Climate;

#[allow(unused_variables)]
impl ws::GlobalDispatch<wlddm::WlDataDeviceManager, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<wlddm::WlDataDeviceManager>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wlddm::WlDataDeviceManager, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wlddm::WlDataDeviceManager,
        request: wlddm::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        match request {
            wlddm::Request::CreateDataSource { id } => {
                data_init.init(id, ());
            }
            wlddm::Request::GetDataDevice { id, seat } => {
                data_init.init(id, ());
            }
            _ => {}
        };
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &wlddm::WlDataDeviceManager,
        data: &(),
    ) {
    }
}

#[allow(unused_variables)]
impl ws::Dispatch<wlddv::WlDataDevice, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wlddv::WlDataDevice,
        request: wlddv::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        // TODO
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &wlddv::WlDataDevice,
        data: &(),
    ) {
    }
}

#[allow(unused_variables)]
impl ws::Dispatch<wlds::WlDataSource, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wlds::WlDataSource,
        request: wlds::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        // TODO
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &wlds::WlDataSource,
        data: &(),
    ) {
    }
}
