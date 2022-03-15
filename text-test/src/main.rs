extern crate ab_glyph as ab;
use ab::Font;
extern crate thundr as th;
use thundr::{CreateInfo, MemImage, SurfaceType, Thundr};

extern crate sdl2;
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
};

use std::collections::HashMap;

struct Glyph {
    /// The thundr image backing this glyph
    g_image: th::Image,
    g_width: usize,
    g_height: usize,
}

struct FontInstance<'a> {
    f_font: ab::FontRef<'a>,
    /// Hashmap of glyphs to look up to find the thundr resources
    f_glyphs: HashMap<char, Glyph>,
    f_scale: f32,
}

impl<'a> FontInstance<'a> {
    fn create_glyph(&mut self, thund: &mut Thundr, c: char) {
        assert!(!self.f_glyphs.contains_key(&c));

        let ab_glyph: ab::Glyph = self
            .f_font
            .glyph_id(c)
            .with_scale_and_position(self.f_scale, ab::point(0.0, 0.0));
        let outline = self.f_font.outline_glyph(ab_glyph).unwrap();
        let bounds = outline.px_bounds();

        let width = bounds.width() as usize;
        let height = bounds.height() as usize;

        let mut img = vec![Pixel(0, 0, 0, 0); width * height].into_boxed_slice();

        outline.draw(|x, y, c| {
            img[y as usize * width + x as usize] = Pixel(255, 255, 255, (c * 255.0) as u8)
        });

        let mimg = MemImage::new(img.as_ptr() as *mut u8, 4, width, height);
        let th_image = thund.create_image_from_bits(&mimg, None).unwrap();

        // Create a new glyph for this UTF-8 character
        let glyph = Glyph {
            g_image: th_image,
            g_width: width,
            g_height: height,
        };

        self.f_glyphs.insert(c, glyph);
    }

    fn create_surface_for_char(
        &mut self,
        thund: &mut Thundr,
        c: char,
        pos: (f32, f32),
    ) -> th::Surface {
        // If we have not imported this glyph, make it now
        if !self.f_glyphs.contains_key(&c) {
            self.create_glyph(thund, c);
        }

        let glyph = self
            .f_glyphs
            .get(&c)
            .expect("Bug: glyph should have been created already");
        let mut surf =
            thund.create_surface(pos.0, pos.1, glyph.g_width as f32, glyph.g_height as f32);
        thund.bind_image(&mut surf, glyph.g_image.clone());

        return surf;
    }
}

#[repr(C)]
#[derive(Clone)]
struct Pixel(u8, u8, u8, u8);

fn main() {
    let mut inst = FontInstance {
        f_font: ab::FontRef::try_from_slice(include_bytes!("../century_gothic.otf"))
            .expect("Could not find font file"),
        f_glyphs: HashMap::new(),
        f_scale: 150.0,
    };

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

    let bg_surf = inst.create_surface_for_char(&mut thund, 'G', (0.0, 0.0));

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
