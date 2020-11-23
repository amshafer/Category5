// Implementation of the wl_pointer interface
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::Main;
use ws::protocol::wl_pointer;

// Called by the wayland filter anytime the client
// makes a request. This does nothing
pub fn wl_pointer_handle_request(req: wl_pointer::Request,
                                 _pointer: Main<wl_pointer::WlPointer>)
{
    match req {
        wl_pointer::Request::Release => {},
        _ => {},
    }
}
