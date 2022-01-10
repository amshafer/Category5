//! # Category5
//!
//! Here we define the toplevel struct for our compositor. It spins up two
//! child threads for `ways` and `vkcomp` respectively and creates a
//! channel for them to pass atmosphere data across.
//!
//! * `ways` - wayland server.
//! * `thundr` - a vulkan toolkit for drawing surfaces.
//! * `vkcomp` - the compositor. It is a window manager that translates
//!   windows from `ways` into `thundr` commands.
//! * `input` - libinput handler. Specific to `ways`, it reacts to user
//!   input and updates the wayland clients
//! * `atmosphere` - A double-buffered database shared by `ways` and `vkcomp`.

// Austin Shafer - 2020
#![allow(non_camel_case_types)]
#[macro_use]
extern crate atmos_gen;
#[macro_use]
extern crate bitflags;

extern crate lazy_static;
extern crate utils;

mod category5;
use category5::Category5;

use std::time::SystemTime;

// This should remain completely safe.
fn main() {
    let mut storm = Category5::spin();

    println!("Begin render loop...");
    let start = SystemTime::now();
    storm.run_forever();
    let end = SystemTime::now();

    println!(
        "uptime: {}",
        end.duration_since(start).unwrap().as_secs_f32()
    );
}
