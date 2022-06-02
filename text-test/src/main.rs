extern crate freetype as ft;
extern crate thundr as th;
use thundr::{CreateInfo, MemImage, SurfaceType, Thundr};
extern crate harfbuzz_rs as hb;
extern crate harfbuzz_sys as hb_sys;

extern crate sdl2;
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
};

#[repr(C)]
#[derive(Clone)]
struct Pixel(u8, u8, u8, u8);

// Define this ourselves since hb crate doesn't do it
extern "C" {
    pub fn hb_ft_font_create_referenced(face: ft::ffi::FT_Face) -> *mut hb_sys::hb_font_t;
}

struct Cursor {
    /// The index into the harfbuzz data arrays
    c_i: usize,
    /// The X position of the pen
    c_x: f32,
    /// The Y position of the pen
    c_y: f32,
    /// The minimum width for line wrap
    /// This is the left side of the layout bounding box
    c_min: f32,
    /// The max width before line wrapping
    /// This is the right side of the layout bounding box
    c_max: f32,
}

struct Glyph {
    /// The thundr image backing this glyph.
    /// This will be none if the glyph does not have an outline
    /// which happens if it's a space.
    g_image: Option<th::Image>,
    g_bitmap_size: (f32, f32),
    g_bitmap_left: f32,
    g_bitmap_top: f32,
    _g_metrics: ft::GlyphMetrics,
}

/// Returns (x_offset, y_offset, x_advance, y_advance)
fn scale_hb_positions(position: &hb::GlyphPosition) -> (f32, f32, f32, f32) {
    // (hb_position_t * font_point_size) / (units / em)
    let buzz_scale = 64.0;
    let x_offset = position.x_offset as f32 / buzz_scale;
    let y_offset = position.y_offset as f32 / buzz_scale;
    let x_advance = position.x_advance as f32 / buzz_scale;
    let y_advance = position.y_advance as f32 / buzz_scale;

    (x_offset, y_offset, x_advance, y_advance)
}

struct FontInstance<'a> {
    _f_freetype: ft::Library,
    /// The font reference for our rasterizer
    f_ft_face: ft::Face,
    /// Our rustybuzz font face (see harfbuzz docs)
    f_hb_font: hb::Owned<hb::Font<'a>>,
    /// Map of glyphs to look up to find the thundr resources
    /// The ab::GlyphId is really just an index into this. That's all
    /// glyph ids are, is the index of the glyph in the font.
    f_glyphs: Vec<Option<Glyph>>,
}

impl<'a> FontInstance<'a> {
    fn new(font_path: &str, dpi: u32, point_size: f32) -> Self {
        let ft_lib = ft::Library::init().unwrap();
        let mut ft_face: ft::Face = ft_lib.new_face(font_path, 0).unwrap();
        let hb_font = unsafe {
            let raw_font =
                hb_ft_font_create_referenced(ft_face.raw_mut() as *mut ft::ffi::FT_FaceRec);
            hb::Owned::from_raw(raw_font)
        };

        // set our font size
        // The sizes come in 1/64th of a point. See the tutorial. Zeroes
        // default to matching that size, and defaults to 72 dpi
        // TODO: account for display info
        ft_face
            .set_char_size(point_size as isize * 64, 0, 0, dpi)
            .expect("Could not set freetype char size");

        Self {
            _f_freetype: ft_lib,
            f_ft_face: ft_face,
            f_hb_font: hb_font,
            f_glyphs: Vec::new(),
        }
    }

    fn create_glyph(&mut self, thund: &mut Thundr, id: u16) -> Glyph {
        self.f_ft_face
            .load_glyph(id as u32, ft::face::LoadFlag::DEFAULT)
            .unwrap();
        let glyph = self.f_ft_face.glyph();
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
            _g_metrics: glyph.metrics(),
        }
    }

    fn ensure_glyph_exists(&mut self, thund: &mut Thundr, id: u16) {
        // If we have not imported this glyph, make it now
        while id as usize >= self.f_glyphs.len() {
            self.f_glyphs.push(None);
        }

        if self.f_glyphs[id as usize].is_none() {
            self.f_glyphs[id as usize] = Some(self.create_glyph(thund, id));
        }
    }

    fn create_surface_for_char(
        &mut self,
        thund: &mut Thundr,
        id: u16,
        pos: (f32, f32),
    ) -> th::Surface {
        self.ensure_glyph_exists(thund, id);
        let glyph = self.f_glyphs[id as usize]
            .as_ref()
            .expect("Bug: Glyph not created for this character");
        let mut surf =
            thund.create_surface(pos.0, pos.1, glyph.g_bitmap_size.0, glyph.g_bitmap_size.1);
        if let Some(image) = glyph.g_image.as_ref() {
            thund.bind_image(&mut surf, image.clone());
        }

        return surf;
    }

    /// returns if we should halt due to out of characters
    fn for_one_line(
        &mut self,
        thund: &mut Thundr,
        list: &mut th::SurfaceList,
        cursor: &mut Cursor,
        _text: &str,
        infos: &[hb::GlyphInfo],
        positions: &[hb::GlyphPosition],
    ) {
        let mut end_index = cursor.c_i;
        let mut last_word = end_index + 1;
        let mut line_pos = cursor.c_x;

        // First find the last glyph we should include on this line
        for i in cursor.c_i..infos.len() {
            let glyph_id = infos[i].codepoint as u16;
            let (_, _, x_advance, _) = scale_hb_positions(&positions[i]);

            // Move the cursor
            line_pos += x_advance;
            end_index = i + 1;

            // check for word breaks
            // For now this is just checking for spaces
            // TODO: use something smarter
            if self.f_ft_face.get_char_index(' ' as u32 as usize) == glyph_id as u32 {
                last_word = end_index;
            }

            // Check for newlines
            // gross, we have to convert to usize through u32 :(
            if self.f_ft_face.get_char_index('\n' as u32 as usize) == glyph_id as u32 {
                last_word = end_index;
                break;
            }

            // Check if we have exceeded the line width. if so, then this line ends
            // at the last known word break (last_word)
            if line_pos >= cursor.c_max {
                break;
            }
        }

        // Now do the above for real and commit it to the surface list
        for i in cursor.c_i..last_word {
            // move to the next char
            cursor.c_i += 1;

            let glyph_id = infos[i].codepoint as u16;
            self.ensure_glyph_exists(thund, glyph_id);
            let glyph = self.f_glyphs[glyph_id as usize]
                .as_ref()
                .expect("Bug: No Glyph created for this character");

            let (x_offset, y_offset, x_advance, y_advance) = scale_hb_positions(&positions[i]);

            let offset = (
                cursor.c_x + x_offset + glyph.g_bitmap_left,
                cursor.c_y + y_offset - glyph.g_bitmap_top,
            );

            let bg_surf = self.create_surface_for_char(thund, glyph_id, offset);
            list.push(bg_surf.clone());

            // Move the cursor
            cursor.c_x += x_advance;
            cursor.c_y += y_advance;
        }
    }

    /// Kicks off layout calculation and text rendering for a paragraph. Increments
    /// the position of the cursor as it goes.
    fn for_each_text_block(
        &mut self,
        thund: &mut Thundr,
        list: &mut th::SurfaceList,
        cursor: &mut Cursor,
        text: &str,
        infos: &[hb::GlyphInfo],
        positions: &[hb::GlyphPosition],
    ) {
        let line_space = self.f_ft_face.size_metrics().unwrap().height / 64;

        loop {
            self.for_one_line(thund, list, cursor, text, infos, positions);
            // Move down to the next line
            cursor.c_x = cursor.c_min;
            cursor.c_y += line_space as f32;

            // Break out of this text item span if we are at the end of the infos
            if cursor.c_i >= infos.len() {
                return;
            }
        }
    }

    /// This is the big text drawing function
    ///
    /// Our Font instance is going to use the provided Thundr context to
    /// create surfaces and lay them out. It's going to update the surface
    /// list with them along the way.
    ///
    /// The cursor argument allows for itemizing runs of different fonts. The
    /// text layout creation will continue at that point.
    fn layout_text(
        &mut self,
        thund: &mut Thundr,
        list: &mut th::SurfaceList,
        cursor: &mut Cursor,
        text: &str,
    ) {
        // Set up our HarfBuzz buffers
        let buffer = hb::UnicodeBuffer::new().add_str(text);

        // Now the big call to get the shaping information
        let glyph_buffer = hb::shape(&self.f_hb_font, buffer, Vec::with_capacity(0).as_slice());
        let infos = glyph_buffer.get_glyph_infos();
        let positions = glyph_buffer.get_glyph_positions();

        self.for_each_text_block(thund, list, cursor, text, infos, positions);
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

    let surf_type = SurfaceType::SDL2(&video_subsystem, &window);

    let info = CreateInfo::builder().surface_type(surf_type).build();
    let mut thund = Thundr::new(&info).unwrap();

    let mut inst = FontInstance::new("./Ubuntu-Regular.ttf", thund.get_dpi() as u32, 9.0);
    let text = "But I must explain to you how all this mistaken idea of reprobating pleasure and extolling pain arose. To do so, I will give you a complete account of the system, and expound the actual teachings of the great explorer of the truth, the master-builder of human happiness.  No one rejects, dislikes or avoids pleasure itself, because it is pleasure, but because those who do not know how to pursue pleasure rationally encounter consequences that are extremely painful. Nor again is there anyone who loves or pursues or desires to obtain pain of itself, because it is pain, but occasionally circumstances occur in which toil and pain can procure him some great pleasure. To take a trivial example, which of us ever undertakes laborious physical exercise, except to obtain some advantage from it? But who has any right to find fault with a man who chooses to enjoy a pleasure that has no annoying consequences, or one who avoids a pain that produces no resultant pleasure? [33] On the other hand, we denounce with righteous indignation and dislike men who are so beguiled and demoralized by the charms of pleasure of the moment, so blinded by desire, that they cannot foresee the pain and trouble that are bound to ensue; and equal blame belongs to those who fail in their duty through weakness of will, which is the same as saying through shrinking from toil and pain. These cases are perfectly simple and easy to distinguish. In a free hour, when our power of choice is untrammeled and when nothing prevents our being able to do what we like best, every pleasure is to be welcomed and every pain avoided. But in certain circumstances and owing to the claims of duty or the obligations of business it will frequently occur that pleasures have to be repudiated and annoyances accepted. The wise man therefore always holds in these matters to this principle of selection: he rejects pleasures to secure other greater pleasures, or else he endures pains to avoid worse pains.";

    // ----------- create list of surfaces
    let mut list = thundr::SurfaceList::new();

    // This is how far we have advanced on a line
    let mut cursor = Cursor {
        c_i: 0,
        c_x: 0.0,
        c_y: 100.0,
        c_min: 0.0,
        c_max: 800.0,
    };
    inst.layout_text(&mut thund, &mut list, &mut cursor, text);

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
