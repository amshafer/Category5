// A basic test of libinput in FreeBSD
//
// This does rely on udev, which in bsd is served
// by the libudev-devd conversion library

extern crate input;
extern crate udev;
extern crate libc;

use udev::{Enumerator,Context};
use input::{Libinput,LibinputInterface};
use input::event::Event;

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
	println!("Opening device {:?}", path);
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
		println!("Returning raw fd {}", fd);
		Ok(fd)
	    },
	    Err(e) => {
                // leave this in, it gives great error msgs
                println!("Error on opening {:?}", e);
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

fn main() {
    // Make a new context for ourselves
    let uctx = Context::new().unwrap();

    // Here we want to get a list of all of the
    // detected devices, which is what the enumerator
    // does.
    let mut udev_enum = Enumerator::new(&uctx).unwrap();
    let devices = udev_enum.scan_devices().unwrap();

    println!("Printing all input devices:");
    for dev in devices {
	println!(" - {:?}", dev.syspath());
    }

    let kit: Inkit = Inkit { _inner: 0 };
    let mut libin = Libinput::new_from_udev(kit, &uctx);

    // we need to choose a "seat" for udev to listen on
    // the default seat is seat0, which is all input devs
    libin.udev_assign_seat("seat0").unwrap();

    loop {
        // dispatch will grab the latest available data
        // from the devices and perform libinputs internal
        // (time sensitive) operations on them
	libin.dispatch().unwrap();

        // TODO: need to fix this wrapper
	let ev = libin.next();
	if !ev.is_none() {
	    println!("Event: {:?}", ev);
	}
    }
}
