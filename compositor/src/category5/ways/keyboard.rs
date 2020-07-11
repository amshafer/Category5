// Implementation of the wl_seat interface
//
// This represents a group of input devices, it is in
// charge of provisioning the keyboard and pointer.
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::Main;
use ws::protocol::wl_keyboard;

pub fn wl_keyboard_handle_request(req: wl_keyboard::Request,
                                  _keyboard: Main<wl_keyboard::WlKeyboard>)
{
    match req {
        _ => {},
    }
}
