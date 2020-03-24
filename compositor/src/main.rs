// A vulkan-backed desktop compositor for FreeBSD
//
// Austin Shafer - 2020
#![allow(non_camel_case_types)]
extern crate ash;
extern crate cgmath;
#[macro_use]
extern crate memoffset;

extern crate bincode;
extern crate serde;
extern crate image;

mod category5;
use category5::Category5;

use std::time::SystemTime;

// This should remain completely safe.
fn main() {
    // read our image
    let img =
        image::open("/home/ashafer/git/compositor_playground/hurricane.png")
        .unwrap()
        .to_rgba();
    let pixels: Vec<u8> = img.into_vec();

    let mut storm = Category5::spin();

    storm.set_background_from_mem(
        pixels,
        // dimensions of the texture
        512,
        512,
    );

    println!("Begin render loop...");
    let start = SystemTime::now();
    storm.run_forever();
    let end = SystemTime::now();

    println!("uptime: {}",
             end.duration_since(start).unwrap().as_secs_f32()
    );
}
