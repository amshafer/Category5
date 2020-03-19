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

mod vkcomp;
mod ways;

use vkcomp::Renderer;
use cgmath::Vector2;
use ash::vk;

use std::time::SystemTime;

static WINDOW_COUNT: u32 = 10;

// This should remain completely safe.
fn main() {
    // If the user passes an argument 'timed', then we should
    // exit after a short bit and print the FPS
    let args: Vec<String> = std::env::args().collect();
    let mut run_forever = true;
    if args.contains(&String::from("timed")) {
        run_forever = false;
    }

    // creates a context, swapchain, images, and others
    let mut rend = Renderer::new();
    // initialize the pipeline, renderpasses, and display engine
    rend.setup();

    let img = image::open("/home/ashafer/git/compositor_playground/bsd.png")
        .unwrap()
        .to_rgba();
    let pixels: Vec<u8> = img.into_vec();

    for i in 0..WINDOW_COUNT {
        rend.add_mesh(
            pixels.as_ref(),
            // dimensions of the texture
            vk::Extent2D {
                width: 512,
                height: 468,
            },
            // size of the window
            vk::Extent2D {
                width: 512,
                height: 512,
            },
            Vector2::new(300 + i * 55, 200 + i * 35),
            // minimum z + inter-window distance * window num
            0.005 + 0.01 * i as f32, // depth
        );
    }

    // read our image

    let img = image::open("/home/ashafer/git/compositor_playground/hurricane.png")
        .unwrap()
        .to_rgba();
    let pixels: Vec<u8> = img.into_vec();

    rend.set_background(
        pixels.as_ref(),
        // dimensions of the texture
        vk::Extent2D {
            width: 512,
            height: 512,
        }
    );

    //rend.record_cbufs();

    println!("Begin render loop...");
    let start = SystemTime::now();

    let runtime = 1000;
    let mut iterations = 0;
    while run_forever || iterations < runtime {
        // Record the cbufs for the next frame
        rend.record_next_cbuf();
        // draw a frame to be displayed
        rend.start_frame();
        // present our frame to the screen
        rend.present();
        iterations += 1;
    }
    let end = SystemTime::now();

    println!("Rendering {} iterations took {:?}",
             runtime,
             end.duration_since(start)
    );
    println!("FPS: {}",
             iterations as f32 / end.duration_since(start)
             .unwrap()
             .as_secs_f32()
    );
}
