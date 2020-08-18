// Implementation of the wl_seat interface
//
// This represents a group of input devices, it is in
// charge of provisioning the keyboard and pointer.
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::Main;
use ws::protocol::{wl_seat, wl_keyboard};
use ws::protocol::wl_seat::Capability;

use crate::category5::utils::WindowId;
use super::keyboard::wl_keyboard_handle_request;

// A collection of protocol objects available to a user
//
// This does not represent a physical seat made of real input
// devices, but rather a set of wayland objects which we use
// to send events to the user
#[allow(dead_code)]
pub struct Seat {
    // The id of the client this seat belongs to
    pub s_id: WindowId,
    // the seat object itself
    pub s_seat: Main<wl_seat::WlSeat>,
    // wl_keyboard handle
    pub s_keyboard: Option<Main<wl_keyboard::WlKeyboard>>,
}

impl Seat {
    // creates an empty seat
    //
    // Also send the capabilities event to let the client know
    // what input methods are ready
    pub fn new(id: WindowId, seat: Main<wl_seat::WlSeat>) -> Seat {
        // broadcast the types of input we have available
        // TODO: don't just default to keyboard + mouse
        //seat.capabilities(Capability::Keyboard | Capability::Pointer);
        seat.capabilities(Capability::Keyboard);

        Seat {
            s_id: id,
            s_seat: seat,
            s_keyboard: None,
        }
    }

    // Handle client requests
    //
    // This basically just creates and registers the different
    // input-related protocols, such as wl_keyboard
    pub fn handle_request(&mut self,
                          req: wl_seat::Request,
                          _seat: Main<wl_seat::WlSeat>)
    {
        match req {
            wl_seat::Request::GetKeyboard { id } => {
                id.quick_assign(move |k, r, _| {
                    wl_keyboard_handle_request(r, k);
                });

                // add the keyboard to this seat
                self.s_keyboard = Some(id);
            },
            _ => unimplemented!("Did not recognize the request"),
        }
    }
}
