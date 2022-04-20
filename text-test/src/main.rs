extern crate rusttype as rt;
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

struct Glyph {
    /// The thundr image backing this glyph.
    /// This will be none if the glyph does not have an outline
    /// which happens if it's a space.
    g_image: Option<th::Image>,
    g_width: f32,
    g_height: f32,
}

struct FontInstance<'a> {
    /// The font reference for our rasterizer
    f_font: rt::Font<'a>,
    /// Font file raw contents. This is held for f_face.
    f_data: Vec<u8>,
    /// Our rustybuzz font face (see harfbuzz docs)
    f_face: rb::Face<'a>,
    /// Map of glyphs to look up to find the thundr resources
    /// The ab::GlyphId is really just an index into this. That's all
    /// glyph ids are, is the index of the glyph in the font.
    f_glyphs: Vec<Glyph>,
    f_scale: rt::Scale,
    f_point_size: f32,
}

impl<'a> FontInstance<'a> {
    fn new(font_path: &str, thund: &mut Thundr, point_size: f32) -> Self {
        let font_data = std::fs::read(font_path).unwrap();

        // See the uninit doc page
        let mut inst = unsafe {
            let mut inst = MaybeUninit::<FontInstance>::uninit();
            let ptr = inst.as_mut_ptr();

            // Using `write` instead of assignment via `=` to not call `drop` on the
            // old, uninitialized value.
            addr_of_mut!((*ptr).f_data).write(font_data);
            // get a reference to f_data.
            let data = &*addr_of!((*ptr).f_data);
            // Now we can use the above reference to fill in the face and font
            // entries in the struct. Here comes the self reference
            addr_of_mut!((*ptr).f_font)
                .write(rt::Font::try_from_bytes(data).expect("Could not find font file"));
            addr_of_mut!((*ptr).f_face).write(
                rb::Face::from_slice(data, 0).expect("Could not initialize rustybuzz::Face"),
            );
            // Now initialize the boring correct ones
            addr_of_mut!((*ptr).f_glyphs).write(Vec::new());
            addr_of_mut!((*ptr).f_point_size).write(point_size);
            addr_of_mut!((*ptr).f_scale).write(rt::Scale { x: 0.0, y: 0.0 });
            // Finally tell the compiler it can go back to sane rules for
            // borrow tracking.
            inst.assume_init()
        };

        // set our font size
        inst.f_face.set_points_per_em(Some(point_size));

        inst.f_scale = rt::Scale {
            x: point_size,
            y: point_size,
        };

        // Create a thundr surface for every glyph in this font
        for i in 0..inst.f_font.glyph_count() {
            assert!(inst.f_glyphs.len() == i);
            let glyph = inst.create_glyph(thund, rt::GlyphId(i as u16));
            inst.f_glyphs.push(glyph);
        }

        return inst;
    }

    fn create_glyph(&mut self, thund: &mut Thundr, id: rt::GlyphId) -> Glyph {
        let rt_glyph = self.f_font.glyph(id).scaled(self.f_scale);
        let hmetrics = rt_glyph.h_metrics();
        let vmetrics = self.f_font.v_metrics(self.f_scale);
        // TODO: add gap fields from the metrics
        let mut width = hmetrics.advance_width;
        let mut height = vmetrics.ascent - vmetrics.descent;

        // if there is no outline for a glyph, then it does not have any
        // contents. In this case we just don't attach an image and record
        // the bounds
        let positioned_glyph = rt_glyph.positioned(rt::Point { x: 0.0, y: 0.0 });
        let th_image = match positioned_glyph.pixel_bounding_box() {
            Some(bounds) => {
                // Regrab the size, since we want the size of the glyph
                // to use for a) the surface size, and b) the image size
                width = bounds.width() as f32;
                height = bounds.height() as f32;
                let mut img = vec![Pixel(0, 0, 0, 0); (width * height) as usize].into_boxed_slice();

                positioned_glyph.draw(|x, y, c| {
                    img[(y * width as u32 + x) as usize] = Pixel(255, 255, 255, (c * 255.0) as u8)
                });

                let mimg =
                    MemImage::new(img.as_ptr() as *mut u8, 4, width as usize, height as usize);
                Some(thund.create_image_from_bits(&mimg, None).unwrap())
            }
            None => None,
        };

        // Create a new glyph for this UTF-8 character
        Glyph {
            g_image: th_image,
            g_width: width,
            g_height: height,
        }
    }

    fn get_glyph_bounds(&self, id: rt::GlyphId) -> (f32, f32) {
        let glyph = &self.f_glyphs[id.0 as usize];
        (glyph.g_width, glyph.g_height)
    }

    fn create_surface_for_char(
        &mut self,
        thund: &mut Thundr,
        id: rt::GlyphId,
        pos: (f32, f32),
    ) -> th::Surface {
        let glyph = &self.f_glyphs[id.0 as usize];
        let mut surf =
            thund.create_surface(pos.0, pos.1, glyph.g_width as f32, glyph.g_height as f32);
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
        let vmetrics = self.f_font.v_metrics(self.f_scale);
        // Convert from rusttype to harfbuzz sizing
        let buzz_scale = self.f_font.units_per_em() as f32 / self.f_scale.x;

        // for each UTF-8 code point in the string
        for i in 0..glyph_buffer.len() {
            let glyph_id = rt::GlyphId(infos[i].glyph_id as u16);
            let glyph_raw = self.f_font.glyph(glyph_id);

            let x_offset = positions[i].x_offset as f32 / buzz_scale;
            let y_offset = positions[i].y_offset as f32 / buzz_scale;

            let glyph = glyph_raw.scaled(self.f_scale).positioned(rt::Point {
                x: x_offset,
                y: y_offset,
            });

            if let Some(bb) = glyph.pixel_bounding_box() {
                let offset = (
                    cursor.0 + x_offset + bb.min.x as f32,
                    cursor.1 + y_offset + vmetrics.ascent + bb.min.y as f32,
                );

                let bg_surf = self.create_surface_for_char(thund, glyph_id, offset);
                list.push(bg_surf.clone());
            }

            // Move the cursor
            //
            // We have to divide the coordinates that rustybuzz gives back to us since
            // it will multiply it by 64. That's basically its way of doing subpixel precision,
            // we are in charge of reducing it back to pixel sizes
            // https://stackoverflow.com/questions/50292283/units-used-by-hb-position-t-in-harfbuzz
            cursor.0 += positions[i].x_advance as f32 / buzz_scale;
            cursor.1 += positions[i].y_advance as f32 / buzz_scale;
        }
    }
}

#[repr(C)]
#[derive(Clone)]
struct Pixel(u8, u8, u8, u8);

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

    let mut inst = FontInstance::new("./Ubuntu-Regular.ttf", &mut thund, 30.0);
    let text = "Almost before we knew it, we had left the ground.";

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
