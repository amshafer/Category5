// The input subsystem
// This can either be hci or automated
//
// Austin Shafer - 2020

// Note that when including this file you need to use
// ::input::*, because the line below imports an
// external input crate.
#![allow(dead_code)]
extern crate input;
extern crate udev;
extern crate nix;

pub mod event;
use event::*;
use crate::category5::utils::{timing::*, logging::LogLevel};
use crate::log;

use udev::{Enumerator,Context};
use input::{Libinput,LibinputInterface};
use input::event::Event;
use input::event::pointer::PointerEvent;
use input::event::keyboard::KeyboardEvent;

use std::fs::{File,OpenOptions};
use std::path::Path;
use std::os::unix::io::RawFd;
use std::os::unix::io::{AsRawFd,IntoRawFd,FromRawFd};
use std::os::unix::fs::OpenOptionsExt;

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
pub struct Input {
    // The udev context
    uctx: Context,
    // libinput context
    libin: Libinput,
}

impl Input {
    // Setup the libinput library from a udev context
    pub fn new() -> Input {
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

        Input {
            uctx: uctx,
            libin: libin,
        }
    }

    // Get a pollable fd
    //
    // This saves power and is monitored by kqueue in
    // the ways event loop
    pub fn get_poll_fd(&mut self) -> RawFd {
        self.libin.as_raw_fd()
    }

    // dispatch will grab the latest available data
    // from the devices and perform libinputs internal
    // (time sensitive) operations on them
    pub fn dispatch(&mut self) {
	self.libin.dispatch().unwrap();
    }

    // Get the next available event from libinput
    //
    // Dispatch should be called before this so libinput can
    // internally read and prepare all events.
    pub fn next_available(&mut self) -> Option<InputEvent> {
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
             Some(Event::Pointer(PointerEvent::Button(b))) => {
                 log!(LogLevel::debug, "pointer button {:?}", b);

                 return Some(InputEvent::left_click(LeftClick {
                     lc_state: b.button_state(),
                 }));
             },
             Some(Event::Keyboard(KeyboardEvent::Key(_))) =>
                 std::process::exit(0),
             Some(e) => log!(LogLevel::error, "Unhandled Input Event: {:?}", e),
             None => (),
         };

        return None;
     }
}
