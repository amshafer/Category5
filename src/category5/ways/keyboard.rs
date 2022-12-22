// Implementation of the wl_seat interface
//
// This represents a group of input devices, it is in
// charge of provisioning the keyboard and pointer.
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use crate::category5::Climate;
use ws::protocol::wl_keyboard;

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wl_keyboard::WlKeyboard, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_keyboard::WlKeyboard,
        request: wl_keyboard::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &(),
    ) {
    }
}
