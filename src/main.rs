// A vulkan-backed desktop compositor for FreeBSD
//
// Austin Shafer - 2020
#![allow(non_camel_case_types)]
#[macro_use]
extern crate atmos_gen;
#[macro_use]
extern crate bitflags;

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

    println!("uptime: {}",
             end.duration_since(start).unwrap().as_secs_f32()
    );
}