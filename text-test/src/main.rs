extern crate freetype as ft;
extern crate thundr as th;
use thundr::{CreateInfo, MemImage, SurfaceType, Thundr};
extern crate rustybuzz as rb;

extern crate sdl2;
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
};

use std::mem::MaybeUninit;
use std::ptr::{addr_of, addr_of_mut};

#[repr(C)]
#[derive(Clone)]
struct Pixel(u8, u8, u8, u8);

struct Glyph {
    /// The thundr image backing this glyph.
    /// This will be none if the glyph does not have an outline
    /// which happens if it's a space.
    g_image: Option<th::Image>,
    g_bitmap_size: (f32, f32),
    g_bitmap_left: f32,
    g_bitmap_top: f32,
    g_metrics: ft::GlyphMetrics,
}

struct FontInstance<'a> {
    f_freetype: ft::Library,
    /// The font reference for our rasterizer
    f_font: ft::Face,
    /// Font file raw contents. This is held for f_face.
    f_data: Vec<u8>,
    /// Our rustybuzz font face (see harfbuzz docs)
    f_face: rb::Face<'a>,
    /// Map of glyphs to look up to find the thundr resources
    /// The ab::GlyphId is really just an index into this. That's all
    /// glyph ids are, is the index of the glyph in the font.
    f_glyphs: Vec<Glyph>,
    // The pixel size of the font
    f_scale: f32,
    f_point_size: f32,
}

impl<'a> FontInstance<'a> {
    fn new(font_path: &str, thund: &mut Thundr, point_size: f32) -> Self {
        let font_data = std::fs::read(font_path).unwrap();

        // See the uninit doc page
        let mut inst = unsafe {
            let mut inst = MaybeUninit::<FontInstance>::uninit();
            let ptr = inst.as_mut_ptr();
            let ft_lib = ft::Library::init().unwrap();
            let ft_face = ft_lib.new_face(font_path, 0).unwrap();

            // Using `write` instead of assignment via `=` to not call `drop` on the
            // old, uninitialized value.
            addr_of_mut!((*ptr).f_data).write(font_data);
            // get a reference to f_data.
            let data = &*addr_of!((*ptr).f_data);
            // Now we can use the above reference to fill in the face and font
            // entries in the struct. Here comes the self reference
            addr_of_mut!((*ptr).f_font).write(ft_face);
            addr_of_mut!((*ptr).f_face).write(
                rb::Face::from_slice(data, 0).expect("Could not initialize rustybuzz::Face"),
            );
            // Now initialize the boring correct ones
            addr_of_mut!((*ptr).f_glyphs).write(Vec::new());
            addr_of_mut!((*ptr).f_point_size).write(point_size);
            addr_of_mut!((*ptr).f_scale).write(0.0);
            // Finally tell the compiler it can go back to sane rules for
            // borrow tracking.
            inst.assume_init()
        };

        // set our font size
        // The sizes come in 1/64th of a point. See the tutorial. Zeroes
        // default to matching that size, and defaults to 72 dpi
        // TODO: account for display info
        inst.f_font
            .set_char_size(point_size as isize * 64, 0, 0, 108)
            .expect("Could not set freetype char size");

        inst.f_scale = point_size;

        // Create a thundr surface for every glyph in this font
        for i in 0..inst.f_font.num_glyphs() {
            assert!(inst.f_glyphs.len() == i as usize);
            let glyph = inst.create_glyph(thund, i as usize);
            inst.f_glyphs.push(glyph);
        }

        return inst;
    }

    fn create_glyph(&mut self, thund: &mut Thundr, id: usize) -> Glyph {
        self.f_font
            .load_glyph(id as u32, ft::face::LoadFlag::DEFAULT)
            .unwrap();
        let glyph = self.f_font.glyph();
        glyph
            .render_glyph(ft::render_mode::RenderMode::Normal)
            .unwrap();
        let bitmap = glyph.bitmap();

        // If the glyph does not have a bitmap, it's an invisible character and
        // we shouldn't make an image for it.
        let th_image = if bitmap.rows() > 0 {
            let mut img = vec![Pixel(0, 0, 0, 0); (bitmap.width() * bitmap.rows()) as usize]
                .into_boxed_slice();
            let width = bitmap.width() as usize;

            // So freetype will give us a bitmap, but we need to turn that into a
            // memory image. This loop goes through each [0,255] value in the bitmap
            // and creates a pixel in our shm texture. We then upload that to thundr
            for (i, v) in bitmap.buffer().iter().enumerate() {
                let x = i % width;
                let y = i / width;
                img[y * width + x] = Pixel(255, 255, 255, *v);
            }

            let mimg = MemImage::new(img.as_ptr() as *mut u8, 4, width, bitmap.rows() as usize);

            Some(thund.create_image_from_bits(&mimg, None).unwrap())
        } else {
            None
        };

        // Create a new glyph for this UTF-8 character
        Glyph {
            g_image: th_image,
            g_bitmap_size: (bitmap.width() as f32, bitmap.rows() as f32),
            g_bitmap_left: glyph.bitmap_left() as f32,
            g_bitmap_top: glyph.bitmap_top() as f32,
            g_metrics: glyph.metrics(),
        }
    }

    fn create_surface_for_char(
        &mut self,
        thund: &mut Thundr,
        id: u16,
        pos: (f32, f32),
    ) -> th::Surface {
        let glyph = &self.f_glyphs[id as usize];
        let mut surf =
            thund.create_surface(pos.0, pos.1, glyph.g_bitmap_size.0, glyph.g_bitmap_size.1);
        if let Some(image) = glyph.g_image.as_ref() {
            thund.bind_image(&mut surf, image.clone());
        }

        return surf;
    }

    /// This is the big text drawing function
    ///
    /// Our Font instance is going to use the provided Thundr context to
    /// create surfaces and lay them out. It's going to update the surface
    /// list with them along the way.
    fn layout_text(&mut self, thund: &mut Thundr, list: &mut th::SurfaceList, text: &str) {
        // Set up our HarfBuzz buffers
        let mut buffer = rb::UnicodeBuffer::new();
        buffer.push_str(text);

        // Now the big call to get the shaping information
        let glyph_buffer = rb::shape(&self.f_face, Vec::with_capacity(0).as_slice(), buffer);
        let infos = glyph_buffer.glyph_infos();
        let positions = glyph_buffer.glyph_positions();

        // This is how far we have advanced on a line
        let mut cursor = (0.0, 100.0);

        // for each UTF-8 code point in the string
        for i in 0..glyph_buffer.len() {
            let glyph_id = infos[i].glyph_id as u16;
            let glyph = &self.f_glyphs[glyph_id as usize];

            // Check for newlines
            // gross, we have to convert to usize through u32 :(
            if self.f_font.get_char_index('\n' as u32 as usize) == glyph_id as u32 {
                cursor.0 = 0.0;
                cursor.1 += 50.0;
                continue;
            }

            // (hb_position_t * font_point_size) / (units / em)
            let buzz_scale = self.f_face.units_per_em() as f32 / self.f_scale;
            let x_offset = positions[i].x_offset as f32 / buzz_scale;
            let y_offset = positions[i].y_offset as f32 / buzz_scale;
            let x_advance = positions[i].x_advance as f32 / buzz_scale;
            let y_advance = positions[i].y_advance as f32 / buzz_scale;

            // TODO: something might be wrong here, I'm thinking of glyphs as having
            // a top left placement origin, but the custom may be bottom left? Look
            // into this.
            let offset = (
                cursor.0 + x_offset + glyph.g_bitmap_left,
                cursor.1 + y_offset - glyph.g_bitmap_top,
            );

            let bg_surf = self.create_surface_for_char(thund, glyph_id, offset);
            list.push(bg_surf.clone());

            // Move the cursor
            cursor.0 += x_advance;
            cursor.1 += y_advance;
        }
    }
}

fn main() {
    // SDL goodies
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem
        .window("thundr-test", 1200, 1080)
        .vulkan()
        .resizable()
        .position_centered()
        .build()
        .unwrap();
    let mut event_pump = sdl_context.event_pump().unwrap();

    let surf_type = SurfaceType::SDL2(&window);

    let info = CreateInfo::builder().surface_type(surf_type).build();
    let mut thund = Thundr::new(&info).unwrap();

    let mut inst = FontInstance::new("./Ubuntu-Regular.ttf", &mut thund, 50.0);
    let text = "But I must explain to you how all this mistaken idea";

    // ----------- create list of surfaces
    let mut list = thundr::SurfaceList::new();

    inst.layout_text(&mut thund, &mut list, text);

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
