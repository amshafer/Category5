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
use ws::protocol::wl_seat::Capability;
use ws::protocol::{wl_keyboard, wl_pointer, wl_seat};
use ws::Main;

use super::keyboard::wl_keyboard_handle_request;
use super::pointer::wl_pointer_handle_request;
use crate::category5::input::Input;
use utils::ClientId;

use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::os::unix::io::FromRawFd;
use std::rc::Rc;

/// See the create_global call in `compositor.rs` for the code
/// that adds a seat instance to a `Seat`.
pub struct SeatInstance {
    // the seat object itself
    pub si_seat: Main<wl_seat::WlSeat>,
    // wl_keyboard handle
    pub si_keyboards: Vec<Main<wl_keyboard::WlKeyboard>>,
    // wl_pointer handle
    pub si_pointers: Vec<Main<wl_pointer::WlPointer>>,
}

impl SeatInstance {
    pub fn new(seat: Main<wl_seat::WlSeat>) -> Self {
        Self {
            si_seat: seat,
            si_keyboards: Vec::new(),
            si_pointers: Vec::new(),
        }
    }

    /// Add a keyboard to this seat
    ///
    /// This also sends the modifier event
    fn get_keyboard(&mut self, parent: &mut Seat, keyboard: Main<wl_keyboard::WlKeyboard>) {
        // register our request handler
        keyboard.quick_assign(move |k, r, _| {
            wl_keyboard_handle_request(r, k);
        });

        let input = parent.s_input.borrow();
        // Make a temp fd to share with the client
        #[cfg(target_os = "freebsd")]
        let fd = unsafe {
            libc::shm_open(
                libc::SHM_ANON,
                libc::O_CREAT | libc::O_RDWR | libc::O_EXCL | libc::O_CLOEXEC,
                0o600,
            )
        };
        #[cfg(target_os = "linux")]
        let fd = unsafe {
            let memfd_name = std::ffi::CString::new("cat5_keymap").unwrap();
            libc::memfd_create(memfd_name.as_ptr() as *mut i8, libc::MFD_CLOEXEC)
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
        keyboard.keymap(
            wl_keyboard::KeymapFormat::XkbV1,
            fd,
            input.i_xkb_keymap_name.as_bytes().len() as u32,
        );
        // Advertise the server repeat capabilities. This is needed
        // to make gtk apps not crash. They will check for this event
        // and if it is not found will resort to checking the peripherals
        // schema, which doesn't have a repeat key and causes an abort.
        // That gross behavior aside, the spec does require us to send this.
        // Send 0 to show we don't repeat.
        // as_ref turns the Main into a Resource
        if keyboard.as_ref().version() >= 4 {
            keyboard.repeat_info(0, 0);
        }

        // add the keyboard to this seat
        self.si_keyboards.push(keyboard.clone());

        // If we are in focus, then we should go ahead and generate
        // the enter event
        let atmos = input.i_atmos.borrow();
        if let Some(focus) = atmos.get_client_in_focus() {
            if parent.s_id == focus {
                if let Some(sid) = atmos.get_win_focus() {
                    if let Some(surf) = atmos.get_wl_surface_from_id(sid) {
                        // TODO: use Input::keyboard_enter and fix the refcell order
                        keyboard.enter(
                            parent.s_serial,
                            &surf,
                            Vec::new(), // TODO: update modifiers if needed
                        );
                    }
                }
            }
        }
    }

    /// Register a wl_pointer to this seat
    fn get_pointer(&mut self, parent: &mut Seat, pointer: Main<wl_pointer::WlPointer>) {
        self.si_pointers.push(pointer.clone());
        pointer.quick_assign(move |p, r, _| {
            wl_pointer_handle_request(r, p);
        });

        // If we are in focus, then we should go ahead and generate
        // the enter event
        let input = parent.s_input.borrow();
        let atmos = input.i_atmos.borrow();
        if let Some(sid) = atmos.get_win_focus() {
            if let Some(pointer_focus) = input.i_pointer_focus {
                // check if the surface is the input sys's focus
                if sid == pointer_focus {
                    Input::pointer_enter(&atmos, sid);
                }
            }
        }
    }
}

/// A collection of protocol objects available to a user
///
/// This does not represent a physical seat made of real input
/// devices, but rather a set of wayland objects which we use
/// to send events to the user
///
/// One of these will exist for each client. Because clients (like firefox)
/// may instantiate multiple registries and wl_seats, this has a list
/// of all the seats created by this client.
#[allow(dead_code)]
pub struct Seat {
    // The handle to the input subsystem
    pub s_input: Rc<RefCell<Input>>,
    // The id of the client this seat belongs to
    pub s_id: ClientId,
    // List of all wl_seats and their respective device proxies
    pub s_proxies: Rc<RefCell<Vec<SeatInstance>>>,
    // the serial number for this set of input events
    pub s_serial: u32,
}

impl Seat {
    /// creates an empty seat
    ///
    /// Also send the capabilities event to let the client know
    /// what input methods are ready.
    ///
    /// The wl_seat needs to be added with `add_seat_instance`.
    pub fn new(input: Rc<RefCell<Input>>, id: ClientId) -> Seat {
        Seat {
            s_input: input,
            s_id: id,
            s_proxies: Rc::new(RefCell::new(Vec::new())),
            s_serial: 0,
        }
    }

    /// Add a wl_seat instance to this Seat.
    ///
    /// `Seat` keeps track of all seat objects for a client. A seat
    /// instance needs to be added for every wl_seat global so that
    /// we can accurately track all wl_seats for a client that have
    /// been created.
    pub fn add_seat_instance(&mut self, seat: Main<wl_seat::WlSeat>) {
        // broadcast the types of input we have available
        // TODO: don't just default to keyboard + mouse
        seat.capabilities(Capability::Keyboard | Capability::Pointer);

        self.s_proxies.borrow_mut().push(SeatInstance::new(seat));
    }

    /// Handle client requests
    ///
    /// This basically just creates and registers the different
    /// input-related protocols, such as wl_keyboard
    pub fn handle_request(&mut self, req: wl_seat::Request, seat: Main<wl_seat::WlSeat>) {
        // we need to borrow proxies seperately so we don't borrow self
        let proxies_rc = self.s_proxies.clone();
        let mut proxies = proxies_rc.borrow_mut();
        let si = proxies
            .iter_mut()
            .find(|s| s.si_seat == seat)
            .expect("wl_seat is not known by this Seat");

        match req {
            wl_seat::Request::GetKeyboard { id } => {
                si.get_keyboard(self, id);
            }
            wl_seat::Request::GetPointer { id } => {
                si.get_pointer(self, id);
            }
            _ => unimplemented!("Did not recognize the request"),
        }
    }
}
