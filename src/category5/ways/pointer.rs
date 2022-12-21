// Implementation of the wl_pointer interface
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use crate::category5::Climate;
use ws::protocol::wl_pointer;

// Dispatch<Interface, Userdata>
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
        // TODO: Implement set_cursor
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &(),
    ) {
    }
}
