extern crate thundr;
use thundr::{CreateInfo, MemImage, SurfaceType, Thundr};

extern crate utils;
use std::marker::PhantomData;
use utils::timing::*;

extern crate winit;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

fn main() {
    // NOTE: uncomment me for winit version
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let surf_type = SurfaceType::Display(PhantomData);
    #[cfg(target_os = "macos")]
    let surf_type = SurfaceType::MacOS(&window);

    let info = CreateInfo::builder()
        .enable_traditional_composition()
        .surface_type(surf_type)
        .build();
    let mut thund = Thundr::new(&info).unwrap();

    // ----------- unused surface
    let img = image::open("images/hurricane.png").unwrap().to_rgba();
    let pixels: Vec<u8> = img.into_vec();
    let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 512, 512);
    let mut bg_image = thund.create_image_from_bits(&mimg, None).unwrap();
    bg_image.set_damage(0, 0, 512, 512);
    let mut bg_surf = thund.create_surface(0.0, 0.0, 512.0, 512.0);
    thund.bind_image(&mut bg_surf, bg_image);

    // ----------- cursor creation
    let img = image::open("images/cursor.png").unwrap().to_rgba();
    let pixels: Vec<u8> = img.into_vec();
    let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);
    let mut cursor_image = thund.create_image_from_bits(&mimg, None).unwrap();
    cursor_image.set_damage(0, 0, 64, 64);
    let mut cursor_surf = thund.create_surface(4.0, 4.0, 16.0, 16.0);
    thund.bind_image(&mut cursor_surf, cursor_image);

    // ----------- background creation
    let img = image::open("images/brick.png").unwrap().to_rgba();
    let pixels: Vec<u8> = img.into_vec();
    let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 512, 512);
    let mut bg_image = thund.create_image_from_bits(&mimg, None).unwrap();
    bg_image.set_damage(0, 0, 512, 512);
    //let ws = window.inner_size();
    let ws = thund.get_resolution();
    let mut bg_surf = thund.create_surface(0.0, 0.0, ws.0 as f32, ws.1 as f32);
    thund.bind_image(&mut bg_surf, bg_image);

    // ----------- create list of surfaces
    let mut list = thundr::SurfaceList::new();
    list.push(cursor_surf.clone());
    list.push(bg_surf);

    let mut dx = 2.0;
    let mut dy = 2.0;

    let mut stop = StopWatch::new();

    let mut draw_func = move || {
        // ----------- update the location of the cursor
        let curpos = cursor_surf.get_pos();
        println!("curpos = {:?}", curpos);
        match curpos.0 {
            v if v < 4.0 => dx = 2.0,
            v if v >= ws.0 as f32 - 4.0 => dx = -2.0,
            _ => {}
        };
        match curpos.1 {
            v if v < 4.0 => dy = 2.0,
            v if v >= ws.1 as f32 - 4.0 => dy = -2.0,
            _ => {}
        };

        cursor_surf.set_pos(curpos.0 + dx, curpos.1 + dy);

        stop.start();
        // ----------- Perform draw calls
        thund.draw_frame(&mut list);

        // ----------- Present to screen
        thund.present();
        stop.end();

        println!(
            "Thundr took {:?} ms this frame",
            stop.get_duration().as_millis()
        );
    };

    // ----------- now wait for the app to exit
    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                _ => (),
            },
            Event::RedrawRequested(_) => {
                draw_func();
                *control_flow = ControlFlow::Wait;
                // Queue another frame
                window.request_redraw();
            }
            _ => (),
        }
    });

    //loop {
    //    draw_func();
    //}
}
