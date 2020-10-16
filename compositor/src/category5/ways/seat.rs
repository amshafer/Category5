// Implementation of the wl_seat interface
//
// This represents a group of input devices, it is in
// charge of provisioning the keyboard and pointer.
//
// Austin Shafer - 2020
extern crate libc;
extern crate nix;
use nix::unistd::ftruncate;

extern crate wayland_server as ws;
use ws::Main;
use ws::protocol::{wl_seat,wl_keyboard,wl_pointer};
use ws::protocol::wl_seat::Capability;

use crate::category5::utils::ClientId;
use crate::category5::input::Input;
use super::keyboard::wl_keyboard_handle_request;
use super::pointer::wl_pointer_handle_request;

use std::io::Write;
use std::os::unix::io::FromRawFd;
use std::fs::File;
use std::rc::Rc;
use std::cell::RefCell;

// A collection of protocol objects available to a user
//
// This does not represent a physical seat made of real input
// devices, but rather a set of wayland objects which we use
// to send events to the user
#[allow(dead_code)]
pub struct Seat {
    // The handle to the input subsystem
    pub s_input: Rc<RefCell<Input>>,
    // The id of the client this seat belongs to
    pub s_id: ClientId,
    // the seat object itself
    pub s_seat: Main<wl_seat::WlSeat>,
    // wl_keyboard handle
    pub s_keyboard: Option<Main<wl_keyboard::WlKeyboard>>,
    // wl_pointer handle
    pub s_pointer: Option<Main<wl_pointer::WlPointer>>,
    // the serial number for this set of input events
    pub s_serial: u32,
}

impl Seat {
    // creates an empty seat
    //
    // Also send the capabilities event to let the client know
    // what input methods are ready
    pub fn new(input: Rc<RefCell<Input>>, id: ClientId, seat: Main<wl_seat::WlSeat>)
               -> Seat
    {
        // broadcast the types of input we have available
        // TODO: don't just default to keyboard + mouse
        seat.capabilities(Capability::Keyboard | Capability::Pointer);

        Seat {
            s_input: input,
            s_id: id,
            s_seat: seat,
            s_keyboard: None,
            s_pointer: None,
            s_serial: 0,
        }
    }

    // Add a keyboard to this seat
    //
    // This also sends the modifier event
    fn get_keyboard(&mut self,
                    keyboard: Main<wl_keyboard::WlKeyboard>) {
        let input = self.s_input.borrow();
        // Make a temp fd to share with the client
        let fd = unsafe {
            libc::shm_open(libc::SHM_ANON,
                           libc::O_CREAT|libc::O_RDWR|libc::O_EXCL|libc::O_CLOEXEC,
                           0o600)
        };
        assert!(fd > 0);
        let mut file = unsafe { File::from_raw_fd(fd) };
        // according to the manpage: writes do not extend
        // shm objects, so we need to call ftruncate first
        ftruncate(fd, input.i_xkb_keymap_name.as_bytes().len() as i64)
            .expect("Could not truncate the temp xkb keymap file");
        // write the input systems keymap to our anon file
        file.write(input.i_xkb_keymap_name.as_bytes())
            .expect("Could not write to the temp xkb keymap file");
        file.flush().unwrap();
        // Broadcast our keymap map
        keyboard.keymap(wl_keyboard::KeymapFormat::XkbV1,
                        fd,
                        input.i_xkb_keymap_name.as_bytes().len() as u32
        );

        // add the keyboard to this seat
        self.s_keyboard = Some(keyboard);

        // If we are in focus, then we should go ahead and generate
        // the enter event
        let atmos = input.i_atmos.borrow();
        if let Some(focus) = atmos.get_client_in_focus() {
            if self.s_id == focus {
                Input::keyboard_enter(&atmos, focus);
            }
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

                self.get_keyboard(id);
            },
            wl_seat::Request::GetPointer { id } => {
                self.s_pointer = Some(id.clone());
                id.quick_assign(move |p, r, _| {
                    wl_pointer_handle_request(r, p);
                });
            },
            _ => unimplemented!("Did not recognize the request"),
        }
    }
}
