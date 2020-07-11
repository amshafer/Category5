// Implementation of the wl_seat interface
//
// This represents a group of input devices, it is in
// charge of provisioning the keyboard and pointer.
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::Main;
use ws::protocol::wl_seat;

use super::keyboard::wl_keyboard_handle_request;

pub fn wl_seat_handle_request(req: wl_seat::Request,
                              _seat: Main<wl_seat::WlSeat>)
{
    match req {
        wl_seat::Request::GetKeyboard { id } =>
            id.quick_assign(move |k, r, _| {
                wl_keyboard_handle_request(r, k);
            }),
        _ => {},
    }
}
