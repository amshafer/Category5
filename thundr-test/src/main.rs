extern crate thundr as th;
use thundr::{CreateInfo, MemImage, SurfaceType, Thundr};

extern crate utils;
//use std::marker::PhantomData;
use utils::timing::*;

extern crate sdl2;
use sdl2::{event::Event, keyboard::Keycode};

fn main() {
    // SDL goodies
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem
        .window("thundr-test", 800, 600)
        .vulkan()
        .resizable()
        .position_centered()
        .build()
        .unwrap();
    let mut event_pump = sdl_context.event_pump().unwrap();

    // let surf_type = SurfaceType::Display(PhantomData);
    let surf_type = SurfaceType::SDL2(&window);

    let info = CreateInfo::builder()
        //.enable_traditional_composition()
        .surface_type(surf_type)
        .build();
    let mut thund = Thundr::new(&info).unwrap();

    // ----------- unused surface
    let img = image::open("images/hurricane.png").unwrap().to_bgra8();
    let pixels: Vec<u8> = img.into_vec();
    let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 512, 512);
    let mut bg_image = thund.create_image_from_bits(&mimg, None).unwrap();
    bg_image.set_damage(0, 0, 512, 512);
    let mut bg_surf = thund.create_surface(0.0, 0.0, 512.0, 512.0);
    thund.bind_image(&mut bg_surf, bg_image);

    // ----------- cursor creation
    let img = image::open("images/cursor.png").unwrap().to_bgra8();
    let pixels: Vec<u8> = img.into_vec();
    let mimg = MemImage::new(pixels.as_slice().as_ptr() as *mut u8, 4, 64, 64);
    let mut cursor_image = thund.create_image_from_bits(&mimg, None).unwrap();
    cursor_image.set_damage(0, 0, 64, 64);
    let mut cursor_surf = thund.create_surface(4.0, 4.0, 16.0, 16.0);
    thund.bind_image(&mut cursor_surf, cursor_image);

    // ----------- background creation
    let img = image::open("images/brick.png").unwrap().to_bgra8();
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
        //println!("curpos = {:?}", curpos);
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
        match thund.draw_frame(&mut list) {
            Ok(_) => {}
            Err(th::ThundrError::OUT_OF_DATE) => return,
            Err(e) => panic!("failed to draw frame: {:?}", e),
        };

        // ----------- Present to screen
        match thund.present() {
            Ok(_) => {}
            Err(th::ThundrError::OUT_OF_DATE) => return,
            Err(e) => panic!("failed to draw frame: {:?}", e),
        };
        stop.end();

        //println!(
        //    "Thundr took {:?} ms this frame",
        //    stop.get_duration().as_millis()
        //);
    };

    // ----------- now wait for the app to exit

    'running: loop {
        draw_func();
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                _ => {}
            }
        }
    }
}
