// The input subsystem
// This can either be hci or automated
//
// Austin Shafer - 2020

// Note that when including this file you need to use
// ::input::*, because the line below imports an
// external input crate.
#![allow(dead_code)]
pub mod codes;
pub mod event;

extern crate wayland_server as ws;
extern crate input;
extern crate udev;
extern crate nix;
extern crate xkbcommon;

use ws::protocol::wl_pointer;

use event::*;
use crate::category5::ways::{
    role::Role,
    xdg_shell::xdg_toplevel::ResizeEdge,
};
use crate::category5::utils::{
    timing::*, logging::LogLevel, atmosphere::*, WindowId,
};
use crate::log;

use udev::{Enumerator,Context};
use input::{Libinput,LibinputInterface};
use input::event::Event;
use input::event::pointer;
use input::event::pointer::{ButtonState, PointerEvent};
use input::event::keyboard::{KeyboardEvent, KeyboardEventTrait, KeyState};

use xkbcommon::xkb;
pub use xkbcommon::xkb::{keysyms, Keysym};

use std::fs::{File,OpenOptions};
use std::path::Path;
use std::os::unix::io::RawFd;
use std::os::unix::io::{AsRawFd,IntoRawFd,FromRawFd};
use std::os::unix::fs::OpenOptionsExt;

use std::rc::Rc;
use std::cell::RefCell;

use std::mem::drop;

// This is sort of like a private userdata struct which
// is used as an interface to the systems devices
//
// i.e. this could call consolekit to avoid having to
// be a root user to get raw input.
struct Inkit {
    // For now we don't have anything special to do,
    // so we are just putting a phantom int here since
    // we need to have something.
    _inner: u32,
}

// This is the interface that libinput uses to abstract away
// consolekit and friends.
//
// In our case we just pass the arguments through to `open`.
// We need to use the unix open extensions so that we can pass
// custom flags.
impl LibinputInterface for Inkit {
    // open a device
    fn open_restricted(&mut self, path: &Path, flags: i32)
                       -> Result<RawFd, i32>
    {
	log!(LogLevel::debug, "Opening device {:?}", path);
	match OpenOptions::new()
            // the unix extension's custom_flag field below
            // masks out O_ACCMODE, i.e. read/write, so add
            // them back in
            .read(true)
            .write(true)
            // libinput wants to use O_NONBLOCK
            .custom_flags(flags)
            .open(path)
        {
	    Ok(f) => {
                // this turns the File into an int, so we
                // don't need to worry about the File's
                // lifetime.
		let fd = f.into_raw_fd();
		log!(LogLevel::error, "Returning raw fd {}", fd);
		Ok(fd)
	    },
	    Err(e) => {
                // leave this in, it gives great error msgs
                log!(LogLevel::error, "Error on opening {:?}", e);
                Err(-1)
            },
	}
    }

    // close a device
    fn close_restricted(&mut self, fd: RawFd) {
	unsafe {
            // this will close the file
	    drop(File::from_raw_fd(fd));
	}
    }
}

// This represents an input system
//
// Input is grabbed from the udev interface, but
// any method should be applicable. It just feeds
// the ways and wm subsystems input events
//
// We will also stash our xkb resources here, and
// will consult this before sending out keymaps/syms
pub struct Input {
    pub i_atmos: Rc<RefCell<Atmosphere>>,
    // The udev context
    uctx: Context,
    // libinput context
    libin: Libinput,
    // xkb goodies
    i_xkb_ctx: xkb::Context,
    i_xkb_keymap: xkb::Keymap,
    // this is referenced by Seat, which needs to map and
    // share it with the clients
    pub i_xkb_keymap_name: String,
    // xkb state machine
    i_xkb_state: xkb::State,

    // Tracking info for the modifier keys
    // These keys are sent separately in the modifiers event
    pub i_mod_ctrl: bool,
    pub i_mod_alt: bool,
    pub i_mod_shift: bool,
    pub i_mod_caps: bool,
    pub i_mod_meta: bool,
    pub i_mod_num: bool,

    // Resize tracking
    // When we resize a window we want to batch together the
    // changes and send one configure message per frame
    // The window currently being resized
    // The currently grabbed resizing window is in the atmosphere
    // changes to the window surface to be sent this frame
    pub i_resize_diff: (f64, f64),
    // The surface that the pointer is currently over
    // note that this may be different than the application focus
    pub i_pointer_focus: Option<WindowId>,
}

impl Input {
    // Setup the libinput library from a udev context
    pub fn new(atmos: Rc<RefCell<Atmosphere>>) -> Input {
        // Make a new context for ourselves
        let uctx = Context::new().unwrap();

        // Here we want to get a list of all of the
        // detected devices, which is what the enumerator
        // does.
        let mut udev_enum = Enumerator::new(&uctx).unwrap();
        let devices = udev_enum.scan_devices().unwrap();

        log!(LogLevel::debug, "Printing all input devices:");
        for dev in devices {
            log!(LogLevel::debug, " - {:?}", dev.syspath());
        }

        let kit: Inkit = Inkit { _inner: 0 };
        let mut libin = Libinput::new_from_udev(kit, &uctx);

        // we need to choose a "seat" for udev to listen on
        // the default seat is seat0, which is all input devs
        libin.udev_assign_seat("seat0").unwrap();

        // Create all the components for xkb
        // A description of this can be found in the xkb
        // section of wayland-book.com
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            &"", &"", &"", &"", // These should be env vars
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        ).expect("Could not initialize a xkb keymap");
        let km_name = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);

        let state = xkb::State::new(&keymap);

        Input {
            i_atmos: atmos,
            uctx: uctx,
            libin: libin,
            i_xkb_ctx: context,
            i_xkb_keymap: keymap,
            i_xkb_keymap_name: km_name,
            i_xkb_state: state,
            i_mod_ctrl: false,
            i_mod_alt: false,
            i_mod_shift: false,
            i_mod_caps: false,
            i_mod_meta: false,
            i_mod_num: false,
            i_resize_diff: (0.0, 0.0),
            i_pointer_focus: None,
        }
    }

    // Get a pollable fd
    //
    // This saves power and is monitored by kqueue in
    // the ways event loop
    pub fn get_poll_fd(&mut self) -> RawFd {
        self.libin.as_raw_fd()
    }

    // Processs any pending input events
    //
    // dispatch will grab the latest available data
    // from the devices and perform libinputs internal
    // (time sensitive) operations on them
    // It will then handle all the available input events
    // before returning.
    pub fn dispatch(&mut self) {
	self.libin.dispatch().unwrap();

        // now go through each event
        while let Some(iev) = self.next_available() {
            self.handle_input_event(&iev);
        }
    }

    // Get the next available event from libinput
    //
    // Dispatch should be called before this so libinput can
    // internally read and prepare all events.
    fn next_available(&mut self) -> Option<InputEvent> {
         // TODO: need to fix this wrapper
	 let ev = self.libin.next();
         match ev {
             Some(Event::Pointer(PointerEvent::Motion(m))) => {
                 log!(LogLevel::debug, "moving mouse by ({}, {})",
                          m.dx(), m.dy());

                 return Some(InputEvent::pointer_move(PointerMove {
                     pm_dx: m.dx(),
                     pm_dy: m.dy(),
                 }));
             },
             Some(Event::Pointer(PointerEvent::Axis(a))) => {
                 log!(LogLevel::debug, "scrolling by ({}, {})",
                      a.axis_value(pointer::Axis::Horizontal),
                      a.axis_value(pointer::Axis::Vertical),
                 );

                 return Some(InputEvent::axis(Axis {
                     a_hori_val: a.axis_value(pointer::Axis::Horizontal),
                     a_vert_val: a.axis_value(pointer::Axis::Vertical),
                 }));
             },
             Some(Event::Pointer(PointerEvent::Button(b))) => {
                 log!(LogLevel::debug, "pointer button {:?}", b.button());

                 return Some(InputEvent::click(Click {
                     c_code: b.button(),
                     c_state: b.button_state(),
                 }));
             },
             Some(Event::Keyboard(KeyboardEvent::Key(k))) => {
                 log!(LogLevel::debug, "keyboard event: {:?}", k.key());
                 return Some(InputEvent::key(Key {
                     k_code: k.key(),
                     k_state: k.key_state(),
                 }));
             },
             Some(e) => log!(LogLevel::error, "Unhandled Input Event: {:?}", e),
             None => (),
         };

        return None;
    }

    // Perform a scrolling motion
    //
    fn handle_pointer_axis(&mut self, a: &Axis) {
        let atmos = self.i_atmos.borrow_mut();

        // Find the active window
        if let Some(id) = self.i_pointer_focus {
            // get the seat for this client
            if let Some(cell) = atmos.get_seat_from_window_id(id) {
                let seat = cell.borrow();
                // Get the pointer
                if let Some(pointer) = &seat.s_pointer {
                    let time = get_current_millis();
                    // deliver the axis events, one for each direction
                    if a.a_hori_val > 0.0 {
                        pointer.axis(
                            time,
                            wl_pointer::Axis::HorizontalScroll,
                            a.a_hori_val,
                        );
                    }

                    if a.a_vert_val > 0.0 {
                        pointer.axis(
                            time,
                            wl_pointer::Axis::VerticalScroll,
                            a.a_vert_val,
                        );
                    }
                }
            }
        }
    }

    // Apply any batched input changes to the window dimensions
    pub fn update_shell(&mut self) {
        let mut atmos = self.i_atmos.borrow_mut();
        if let Some(id) = atmos.get_resizing() {
            if let Some(cell) = atmos.get_surface_from_id(id) {
                let surf = cell.borrow();
                match &surf.s_role {
                    Some(Role::xdg_shell_toplevel(ss)) => {
                        // send the xdg configure events
                        ss.borrow_mut()
                            .configure(
                                &mut atmos, &surf,
                                Some((
                                    self.i_resize_diff.0 as f32,
                                    self.i_resize_diff.1 as f32,
                                )),
                            );
                    },
                    _ => (),
                }
            }

            // clear the diff so we can batch more
            self.i_resize_diff = (0.0, 0.0);
        }
    }

    // Generate the wl_keyboard.enter event for id's seat, if it
    // has a keyboard.
    //
    // Atmos is passed since this is called from `atmos.focus_on`,
    // so atmos' rc may be held.
    pub fn keyboard_enter(atmos: &Atmosphere, id: WindowId) {
        let client = atmos.get_owner(id);
        if let Some(cell) = atmos.get_seat_from_client_id(client) {
            let seat = cell.borrow_mut();
            if let Some(keyboard) = &seat.s_keyboard {
                if let Some(surf) = atmos.get_wl_surface_from_id(id) {
                    keyboard.enter(
                        seat.s_serial,
                        &surf,
                        Vec::new(),
                    );
                }
            }
        }
    }

    // Generate the wl_keyboard.leave event for id's seat, if it
    // has a keyboard.
    //
    // Atmos is passed since this is called from `atmos.focus_on`,
    // so atmos' rc may be held.
    pub fn keyboard_leave(atmos: &Atmosphere, id: WindowId) {
        if let Some(cell) = atmos.get_seat_from_window_id(id) {
            let seat = cell.borrow_mut();
            if let Some(keyboard) = &seat.s_keyboard {
                if let Some(surf) = atmos.get_wl_surface_from_id(id) {
                    keyboard.leave(
                        seat.s_serial,
                        &surf,
                    );
                }
            }
        }
    }

    // Generate the wl_pointer.enter event for id's seat, if it
    // has a pointer.
    //
    // Atmos is passed since this may be called from `atmos.focus_on`,
    // so atmos' rc may be held.
    pub fn pointer_enter(atmos: &Atmosphere, id: WindowId) {
        if let Some(cell) = atmos.get_seat_from_window_id(id) {
            let seat = cell.borrow_mut();
            if let Some(pointer) = &seat.s_pointer {
                if let Some(surf) = atmos.get_wl_surface_from_id(id) {
                    let (cx, cy) = atmos.get_cursor_pos();
                    if let Some((sx, sy)) = atmos
                        .global_coords_to_surf(id, cx, cy)
                    {
                        pointer.enter(
                            seat.s_serial,
                            &surf,
                            sx as f64, sy, // surface local coordinates
                        );
                    }
                }
            }
        }
    }

    // Generate the wl_pointer.leave event for id's seat, if it
    // has a pointer.
    //
    // Atmos is passed since this may be called from `atmos.focus_on`,
    // so atmos' rc may be held.
    pub fn pointer_leave(atmos: &Atmosphere, id: WindowId) {
        if let Some(cell) = atmos.get_seat_from_window_id(id) {
            let seat = cell.borrow_mut();
            if let Some(pointer) = &seat.s_pointer {
                if let Some(surf) = atmos.get_wl_surface_from_id(id) {
                    pointer.leave(
                        seat.s_serial,
                        &surf,
                    );
                }
            }
        }
    }

    // Move the pointer
    //
    // Also generates wl_pointer.motion events to the surface
    // in focus if the cursor is on that surface
    fn handle_pointer_move(&mut self, m: &PointerMove) {
        let mut atmos = self.i_atmos.borrow_mut();

        // If a resize is happening then collect the cursor changes
        // to send at the end of the frame
        if atmos.get_resizing().is_some() {
            self.i_resize_diff.0 += m.pm_dx;
            self.i_resize_diff.1 += m.pm_dy;
            return;
        }
        // Update the atmosphere with the new cursor pos
        atmos.add_cursor_pos(m.pm_dx, m.pm_dy);
        // Get the cursor position
        let (cx, cy) = atmos.get_cursor_pos();

        // Get the window the pointer is over
        let focus = atmos.find_window_at_point(cx as f32, cy as f32);
        // If the pointer is over top of a different window, change the
        // pointer focus and send the leave/enter events
        if focus != self.i_pointer_focus {
            if let Some(id) = self.i_pointer_focus {
                Input::pointer_leave(&atmos, id);
            }
            if let Some(id) = focus {
                Input::pointer_enter(&atmos, id);
            }
            self.i_pointer_focus = focus;
        }

        // deliver the motion event
        if let Some(id) = focus {
            if let Some(cell) = atmos.get_seat_from_window_id(id) {
                // get the seat for this client
                let seat = cell.borrow();
                // Get the pointer
                if let Some(pointer) = &seat.s_pointer {
                    // If the pointer is over this surface
                    if let Some((sx, sy)) = atmos
                        .global_coords_to_surf(id, cx, cy)
                    {
                        // deliver the motion event
                        pointer.motion(get_current_millis(), sx, sy);
                    }
                }
            }
        }
    }

    // Does what it says
    //
    // This is the big ugly state machine for processing an input
    // token that was the result of clicking the pointer. We need
    // to find what the cursor is over and perform the appropriate
    // action.
    //
    // If a click is over a background window it is brought into focus
    // clicking on a background titlebar can also start a grab
    fn handle_click_on_window(&mut self, c: &Click) {
        let mut atmos = self.i_atmos.borrow_mut();
        let cursor = atmos.get_cursor_pos();
        // did our click bring a window into focus?
        let mut set_focus = false;

        // first check if we are releasing a grab
        if let Some(id) = atmos.get_grabbed() {
            match c.c_state {
                ButtonState::Released => {
                    log!(LogLevel::debug, "Ungrabbing window {}", id);
                    atmos.ungrab();
                    return;
                },
                _ => (),
            }
        }

        // find the window under the cursor
        let resizing = atmos.get_resizing();
        if resizing.is_some() && c.c_state == ButtonState::Released {
            // We are releasing a resize, and we might not be resizing
            // the same window as find_window_at_point would report
            if let Some(id) = resizing {
                // if on one of the edges start a resize
                if let Some(surf) = atmos.get_surface_from_id(id) {
                    match &surf.borrow_mut().s_role {
                        Some(Role::xdg_shell_toplevel(ss)) => {
                            match c.c_state {
                                // The release is handled above
                                ButtonState::Released => {
                                    log!(LogLevel::debug,
                                         "Stopping resize of {}", id);
                                    ss.borrow_mut().ss_attached_state
                                        .xs_resizing = false;
                                    atmos.set_resizing(None);
                                    // TODO: send final configure here?
                                },
                                // this should never be pressed
                                _ => (),
                            }
                        },
                        // TODO: resizing for other shell types
                        _ => (),
                    }
                }
            }
        } else if let Some(id) = atmos.find_window_at_point(cursor.0 as f32,
                                                            cursor.1 as f32)
        {
            // If the window is not in focus, make it in focus
            if let Some(focus) = atmos.get_window_in_focus() {
                if id != focus && c.c_state == ButtonState::Pressed {
                    // Tell atmos that this is the one in focus
                    atmos.focus_on(Some(id));
                    set_focus = true;
                }
            }

            // do this first here so we don't do it more than once
            let edge = atmos.point_is_on_window_edge(id, cursor.0 as f32,
                                                     cursor.1 as f32);

            // First check if we are over an edge, or if we are resizing
            // and released the click
            if edge != ResizeEdge::None {
                // if on one of the edges start a resize
                if let Some(surf) = atmos.get_surface_from_id(id) {
                    match &surf.borrow_mut().s_role {
                        Some(Role::xdg_shell_toplevel(ss)) => {
                            match c.c_state {
                                ButtonState::Pressed => {
                                    log!(LogLevel::debug,
                                         "Resizing window {}", id);
                                    ss.borrow_mut().ss_attached_state
                                        .xs_resizing = true;
                                    atmos.set_resizing(Some(id));
                                },
                                // releasing is handled above
                                _ => (),
                            }
                        },
                        // TODO: resizing for other shell types
                        _ => (),
                    }
                }
            } else if atmos.point_is_on_titlebar(id, cursor.0 as f32,
                                                 cursor.1 as f32)
            {
                // now check if we are over the titlebar
                // if so we will grab the bar
                match c.c_state {
                    ButtonState::Pressed => {
                        log!(LogLevel::debug, "Grabbing window {}", id);
                        atmos.grab(id);
                    },
                    ButtonState::Released => {
                        log!(LogLevel::debug, "Ungrabbing window {}", id);
                        atmos.ungrab();
                    }
                }
            } else if !set_focus {
                // else the click was over the meat of the window, so
                // deliver the event to the wayland client

                // get the seat for this client
                if let Some(cell) = atmos.get_seat_from_window_id(id) {
                    let seat = cell.borrow_mut();
                    if let Some(pointer) = &seat.s_pointer {
                        // Trigger a button event
                        pointer.button(
                            seat.s_serial,
                            get_current_millis(),
                            c.c_code,
                            match c.c_state {
                                ButtonState::Pressed =>
                                    wl_pointer::ButtonState::Pressed,
                                ButtonState::Released =>
                                    wl_pointer::ButtonState::Released,
                            },
                        );
                    }
                }
            }
        }
    }

    // Handle the user typing on the keyboard
    //
    //
    pub fn handle_keyboard(&mut self, key: &Key) {
        // find the client in use
        let atmos = self.i_atmos.borrow_mut();
        // if there is a window in focus
        if let Some(id) = atmos.get_client_in_focus() {
            // get the seat for this client
            if let Some(cell) = atmos.get_seat_from_client_id(id) {
                let mut seat = cell.borrow_mut();
                if let Some(keyboard) = &seat.s_keyboard {
                    // let xkb keep track of the keyboard state
                    let changed = self.i_xkb_state.update_key(
                        // add 8 to account for differences between evdev and x11
                        key.k_code + 8,
                        match key.k_state {
                            KeyState::Pressed => xkb::KeyDirection::Down,
                            KeyState::Released => xkb::KeyDirection::Up,
                        }
                    );
                    // if any modifiers were touched we should send their event
                    if changed != 0 {
                        // First we need to update our own tracking of what keys are held down
                        self.i_mod_ctrl = self.i_xkb_state.mod_name_is_active(&xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
                        self.i_mod_alt = self.i_xkb_state.mod_name_is_active(&xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
                        self.i_mod_shift = self.i_xkb_state.mod_name_is_active(&xkb::MOD_NAME_SHIFT, xkb::STATE_MODS_EFFECTIVE);
                        self.i_mod_caps = self.i_xkb_state.mod_name_is_active(&xkb::MOD_NAME_CAPS, xkb::STATE_MODS_EFFECTIVE);
                        self.i_mod_meta = self.i_xkb_state.mod_name_is_active(&xkb::MOD_NAME_LOGO, xkb::STATE_MODS_EFFECTIVE);
                        self.i_mod_num = self.i_xkb_state.mod_name_is_active(&xkb::MOD_NAME_NUM, xkb::STATE_MODS_EFFECTIVE);

                        // Now we can serialize the modifiers into a format suitable
                        // for sending to the client
                        let depressed = self.i_xkb_state.serialize_mods(xkb::STATE_MODS_DEPRESSED);
                        let latched = self.i_xkb_state.serialize_mods(xkb::STATE_MODS_LATCHED);
                        let locked = self.i_xkb_state.serialize_mods(xkb::STATE_MODS_LOCKED);
                        let layout = self.i_xkb_state.serialize_layout(xkb::STATE_LAYOUT_LOCKED);

                        // Finally fire the wayland event
                        log!(LogLevel::debug,"Sending modifiers to window {}",id);
                        keyboard.modifiers(
                            seat.s_serial,
                            depressed, latched, locked, layout,
                        );
                    }
                    // give the keycode to the client
                    let time = get_current_millis();
                    let state = map_key_state(key.k_state);
                    log!(LogLevel::debug,"Sending {} key to window {}",
                         key.k_code, id);
                    keyboard.key(seat.s_serial, time, key.k_code, state);

                    // increment the serial for next time
                    seat.s_serial += 1;
                }
            }
        }
        // otherwise the click is over the background, so
        // ignore it
    }

    // Dispatch an arbitrary input event
    //
    // Input events are either handled by us or by the wayland client
    // we need to figure out the appropriate destination and perform
    // the right action.
    pub fn handle_input_event(&mut self, iev: &InputEvent) {
        match iev {
            InputEvent::pointer_move(m) =>
                self.handle_pointer_move(m),
            InputEvent::axis(a) =>
                self.handle_pointer_axis(a),
            InputEvent::click(c) =>
                self.handle_click_on_window(c),
            InputEvent::key(k) =>
                self.handle_keyboard(k),
        }
    }
}
