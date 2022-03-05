extern crate ab_glyph;
use ab_glyph::{point, Font, FontRef, Glyph};
extern crate thundr as th;
use thundr::{CreateInfo, MemImage, SurfaceType, Thundr};

extern crate sdl2;
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
};

#[repr(C)]
#[derive(Clone)]
struct Pixel(u8, u8, u8, u8);

fn main() {
    let font = FontRef::try_from_slice(include_bytes!("../century_gothic.otf"))
        .expect("Could not find font file");

    let q_glyph: Glyph = font
        .glyph_id('G')
        .with_scale_and_position(50.0, point(0.0, 0.0));
    let outline = font.outline_glyph(q_glyph).unwrap();
    let bounds = outline.px_bounds();

    let width = bounds.width() as usize;
    let height = bounds.height() as usize;

    let mut img = vec![Pixel(0, 0, 0, 0); width * height].into_boxed_slice();

    outline.draw(|x, y, c| {
        img[y as usize * width + x as usize] = Pixel(255, 255, 255, (c * 255.0) as u8)
    });

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

    let surf_type = SurfaceType::SDL2(&window);

    let info = CreateInfo::builder().surface_type(surf_type).build();
    let mut thund = Thundr::new(&info).unwrap();

    // ----------- unused surface
    let mimg = MemImage::new(img.as_ptr() as *mut u8, 4, width, height);
    let bg_image = thund.create_image_from_bits(&mimg, None).unwrap();
    let scale = 10.0;
    let mut bg_surf = thund.create_surface(0.0, 0.0, width as f32 * scale, height as f32 * scale);
    thund.bind_image(&mut bg_surf, bg_image);

    // ----------- create list of surfaces
    let mut list = thundr::SurfaceList::new();
    list.push(bg_surf.clone());

    // ----------- now wait for the app to exit

    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Window {
                    timestamp: _,
                    window_id: _,
                    win_event,
                } => match win_event {
                    WindowEvent::Resized { .. } => thund.handle_ood(),
                    _ => {}
                },
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                _ => {}
            }
        }
        // ----------- Perform draw calls
        match thund.draw_frame(&mut list) {
            Ok(_) => {}
            Err(th::ThundrError::OUT_OF_DATE) => continue,
            Err(e) => panic!("failed to draw frame: {:?}", e),
        };

        // ----------- Present to screen
        match thund.present() {
            Ok(_) => {}
            Err(th::ThundrError::OUT_OF_DATE) => continue,
            Err(e) => panic!("failed to draw frame: {:?}", e),
        };
    }
}
