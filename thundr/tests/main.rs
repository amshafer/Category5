extern crate thundr;
use thundr::{CreateInfo, MemImage, SurfaceType, Thundr};

extern crate winit;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

extern crate image as img;

#[test]
fn moving_surface() {
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();

    let surf_type = {
        {
            #[cfg(feature = "xcb")]
            SurfaceType::Xcb(&window)
        }
        {
            #[cfg(feature = "macos")]
            SurfaceType::MacOS(&window)
        }
    };

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
    let ws = window.inner_size();
    let mut bg_surf = thund.create_surface(0.0, 0.0, ws.width as f32, ws.height as f32);
    thund.bind_image(&mut bg_surf, bg_image);

    // ----------- create list of surfaces
    let mut list = thundr::SurfaceList::new();
    list.push(cursor_surf.clone());
    list.push(bg_surf);

    let mut dx = 2.0;
    let mut dy = 2.0;

    let mut frame_count = 0;

    // ----------- now wait for the app to exit
    event_loop.run(move |event, _, control_flow| {
        frame_count += 1;
        if frame_count > 50 {
            return;
        }

        // ----------- update the location of the cursor
        let curpos = cursor_surf.get_pos();
        println!("curpos = {:?}", curpos);
        match curpos.0 {
            v if v < 4.0 => dx = 2.0,
            v if v >= ws.width as f32 - 4.0 => dx = -2.0,
            _ => {}
        };
        match curpos.1 {
            v if v < 4.0 => dy = 2.0,
            v if v >= ws.height as f32 - 4.0 => dy = -2.0,
            _ => {}
        };

        cursor_surf.set_pos(curpos.0 + dx, curpos.1 + dy);

        // ----------- Perform draw calls
        thund.draw_frame(&mut list);

        // ----------- Present to screen
        thund.present();

        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
            } if window_id == window.id() => *control_flow = ControlFlow::Exit,
            _ => (),
        }
    });
}
