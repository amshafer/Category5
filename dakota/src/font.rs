extern crate freetype as ft;
extern crate harfbuzz as hb;
extern crate harfbuzz_sys as hb_sys;

use crate::DakotaId;
use lluvia as ll;

// Define this ourselves since hb crate doesn't do it
extern "C" {
    pub fn hb_ft_font_create_referenced(face: ft::ffi::FT_Face) -> *mut hb_sys::hb_font_t;
}

#[derive(Debug)]
pub struct Cursor {
    /// The index into the harfbuzz data arrays
    pub c_i: usize,
    /// The X position of the pen
    pub c_x: i32,
    /// The Y position of the pen
    pub c_y: i32,
    /// The minimum width for line wrap
    /// This is the left side of the layout bounding box
    pub c_min: i32,
    /// The max width before line wrapping
    /// This is the right side of the layout bounding box
    pub c_max: i32,
}

#[derive(Clone)]
pub struct Glyph {
    /// The thundr image backing this glyph.
    /// This will be none if the glyph does not have an outline
    /// which happens if it's a space.
    pub g_image: Option<th::Image>,
    pub g_bitmap_size: (i32, i32),
    pub g_bitmap_left: i32,
    pub g_bitmap_top: i32,
    _g_metrics: ft::GlyphMetrics,
}

/// Returns (x_offset, y_offset, x_advance, y_advance)
fn scale_hb_positions(position: &hb_sys::hb_glyph_position_t) -> (i32, i32, i32, i32) {
    // (hb_position_t * font_point_size) / (units / em)
    let buzz_scale = 64;
    let x_offset = position.x_offset / buzz_scale;
    let y_offset = position.y_offset / buzz_scale;
    let x_advance = position.x_advance / buzz_scale;
    let y_advance = position.y_advance / buzz_scale;

    (x_offset, y_offset, x_advance, y_advance)
}

/// This struct caches the per-character layout information performed while laying
/// out text.
///
/// This prevents us from recalling freetype and recreating layout nodes and such.
#[derive(Debug, Clone)]
pub struct CachedChar {
    /// The layout node that represents this character
    pub node: DakotaId,
    /// The glyph id to be used to test which character this is
    pub glyph_id: DakotaId,
    /// The raw freetype glyph index
    pub raw_glyph_id: u16,
    /// The final offset calculated by freetype/harfbuzz that we will add to the
    /// cursor when laying out text.
    pub cursor_advance: (i32, i32),
    /// This is the offset from the cursor position to place this char
    pub offset: (i32, i32),
}

/// Instance of a Font
///
/// This refers to the instance of font shaping library context, notably Harfbuzz.
/// This is used to perform shaping.
pub struct FontInstance {
    /// The font reference for our rasterizer
    f_ft_face: ft::Face,
    /// Our rustybuzz font face (see harfbuzz docs)
    ///
    /// Note that this is a raw pointer. This is to work around some
    /// obnoxious lifetime issues. hb::Font<'a> has a lifetime parameter,
    /// which means if we use it this lifetime has to be specified all the way
    /// up to the Dakota object. This isn't acceptable since a lifetime parameter
    /// means Dakota can't be used in environments that require a static lifetime,
    /// so we have to do this annoying dance here to avoid all of that.
    ///
    /// Each time you need a Font object, use hb::Font::from_raw()
    f_hb_raw_font: *mut harfbuzz_sys::hb_font_t,
    /// Map of glyphs to look up to find the thundr resources
    /// The ab::GlyphId is really just an index into this. That's all
    /// glyph ids are, is the index of the glyph in the font.
    f_glyphs: Vec<Option<DakotaId>>,
}

impl FontInstance {
    /// Create a new font
    ///
    /// This is a particular font from a typeface at a
    /// particular size. Size is specified in points.
    pub fn new(ft_lib: &ft::Library, font_path: &str, pixel_size: u32) -> Self {
        let mut ft_face: ft::Face = ft_lib.new_face(font_path, 0).unwrap();
        let raw_font =
            unsafe { hb_ft_font_create_referenced(ft_face.raw_mut() as *mut ft::ffi::FT_FaceRec) };

        ft_face
            .set_pixel_sizes(pixel_size, pixel_size)
            //.set_point_sizes(point_size as u32, point_size as u32)
            .expect("Could not set freetype char size");

        Self {
            f_ft_face: ft_face,
            f_hb_raw_font: raw_font,
            f_glyphs: Vec::new(),
        }
    }

    fn create_glyph(
        &mut self,
        display: &mut th::Display,
        inst: &mut ll::Instance,
        glyphs: &mut ll::Snapshot<Glyph>,
        id: u16,
    ) -> DakotaId {
        let flags = match self.f_ft_face.has_color() {
            true => ft::face::LoadFlag::COLOR,
            false => ft::face::LoadFlag::DEFAULT,
        };
        self.f_ft_face.load_glyph(id as u32, flags).unwrap();
        let glyph = self.f_ft_face.glyph();
        glyph
            .render_glyph(ft::render_mode::RenderMode::Normal)
            .unwrap();
        let bitmap = glyph.bitmap();

        // If the glyph does not have a bitmap, it's an invisible character and
        // we shouldn't make an image for it.
        let th_image = if bitmap.rows() > 0 {
            let width = bitmap.width() as usize;
            let height = bitmap.rows() as usize;
            let mut img: Vec<u8> = std::iter::repeat(0)
                .take(width * height * 4 as usize)
                .collect();

            let pixel_mode = bitmap.pixel_mode().expect("Failed to query pixel mode");

            if pixel_mode == ft::bitmap::PixelMode::Gray {
                // Handle Gray Pixels
                // ------------------
                //
                // So freetype will give us a bitmap, but we need to turn that into a
                // memory image. This loop goes through each [0,255] value in the bitmap
                // and creates a pixel in our shm texture. We then upload that to thundr
                for (i, v) in bitmap.buffer().iter().enumerate() {
                    let x = i % width;
                    let y = i / width;
                    let idx = (y * width + x) * 4;
                    img[idx] = 255;
                    img[idx + 1] = 255;
                    img[idx + 2] = 255;
                    img[idx + 3] = *v;
                }
            } else if pixel_mode == ft::bitmap::PixelMode::Bgra {
                // Handle Colored Pixels
                // ---------------------
                //
                // In this mode if the face supported it we will handle subpixel hinting
                // through colored bitmaps.
                for i in 0..img.len() {
                    let pixel_off = i * 4;
                    let b = bitmap.buffer();
                    // copy the four bgra components into our memimage
                    img[i] = b[pixel_off];
                    img[i + 1] = b[pixel_off + 1];
                    img[i + 2] = b[pixel_off + 2];
                    img[i + 3] = b[pixel_off + 3];
                }
            } else {
                unimplemented!("Unimplemented freetype pixel mode {:?}", pixel_mode);
            }

            Some(
                display
                    .d_dev
                    .create_image_from_bits(
                        img.as_slice(),
                        width as u32,
                        bitmap.rows() as u32,
                        0,
                        None,
                    )
                    .unwrap(),
            )
        } else {
            None
        };

        // Create a new glyph for this UTF-8 character
        let id = inst.add_entity();
        glyphs.set(
            &id,
            Glyph {
                g_image: th_image,
                g_bitmap_size: (bitmap.width(), bitmap.rows()),
                g_bitmap_left: glyph.bitmap_left(),
                g_bitmap_top: glyph.bitmap_top(),
                _g_metrics: glyph.metrics(),
            },
        );

        return id;
    }

    /// Go ahead and create the Glyph for an id in our map
    fn ensure_glyph_exists(
        &mut self,
        display: &mut th::Display,
        inst: &mut ll::Instance,
        glyphs: &mut ll::Snapshot<Glyph>,
        id: u16,
    ) {
        // If we have not imported this glyph, make it now
        while id as usize >= self.f_glyphs.len() {
            self.f_glyphs.push(None);
        }

        if self.f_glyphs[id as usize].is_none() {
            self.f_glyphs[id as usize] = Some(self.create_glyph(display, inst, glyphs, id));
        }
    }

    /// Handle one line of text
    ///
    /// This calls the glyph callback to handle up to one line of text. The
    /// return value is false if the end of a line was not reached by this
    /// text, and true if this function returned because the text is more
    /// than one line long.
    fn for_one_line<F>(
        &mut self,
        display: &mut th::Display,
        cursor: &mut Cursor,
        text: &[CachedChar],
        glyph_callback: &mut F,
    ) -> bool
    where
        F: FnMut(&mut Self, &mut th::Display, &mut Cursor, &CachedChar),
    {
        let mut ret = false;
        let mut end_index = cursor.c_i + 1;
        // The last space separated point
        let mut last_word = end_index;
        // Should we use the above last word?
        let mut line_wrap_needed = false;
        let mut line_pos = cursor.c_x;

        // First find the last glyph we should include on this line
        for i in cursor.c_i..text.len() {
            let glyph_id = text[i].raw_glyph_id;

            // Move the cursor
            line_pos += text[i].cursor_advance.0;
            end_index = i + 1;

            // check for word breaks
            // For now this is just checking for spaces
            // TODO: use something smarter
            if self
                .f_ft_face
                .get_char_index(' ' as u32 as usize)
                .unwrap_or(0)
                == glyph_id as u32
            {
                last_word = end_index;
            }

            // Check for newlines
            // gross, we have to convert to usize through u32 :(
            if self
                .f_ft_face
                .get_char_index('\n' as u32 as usize)
                .unwrap_or(0)
                == glyph_id as u32
            {
                last_word = end_index;
                ret = true;
                break;
            }

            // Check if we have exceeded the line width. if so, then this line ends
            // at the last known word break (last_word)
            if line_pos >= cursor.c_max {
                line_wrap_needed = true;
                ret = true;
                break;
            }
        }

        let end_of_line = if line_wrap_needed {
            last_word
        } else {
            end_index
        };

        // Now do the above for real and commit it to the surface list
        for i in cursor.c_i..end_of_line {
            // move to the next char
            cursor.c_i += 1;

            glyph_callback(self, display, cursor, &text[i]);

            // Move the cursor
            cursor.c_x += text[i].cursor_advance.0;
            cursor.c_y += text[i].cursor_advance.1;
        }

        return ret;
    }

    /// Helper for getting the height of a line of text
    pub fn get_vertical_line_spacing(&self) -> i32 {
        self.f_ft_face.size_metrics().unwrap().height as i32 / 64
    }

    /// Kicks off layout calculation and text rendering for a paragraph. Increments
    /// the position of the cursor as it goes.
    fn for_each_text_block<F>(
        &mut self,
        display: &mut th::Display,
        cursor: &mut Cursor,
        text: &[CachedChar],
        glyph_callback: &mut F,
    ) where
        F: FnMut(&mut Self, &mut th::Display, &mut Cursor, &CachedChar),
    {
        let line_space = self.get_vertical_line_spacing();

        loop {
            if self.for_one_line(display, cursor, text, glyph_callback) {
                // Move down to the next line
                cursor.c_x = cursor.c_min;
                cursor.c_y += line_space;
            }

            // Break out of this text item span if we are at the end of the infos
            if cursor.c_i >= text.len() {
                return;
            }
        }

        // TODO: Add on the width of one space to separate this from any
        // future itemized runs that may come our way
    }

    /// This is the big text drawing function
    ///
    /// The caller will pass in a callback which will get called on a
    /// per-glyph basis to get layout information propogated to it. In reality
    /// this mechanism is purpose built for dakota: dakota wants to be able
    /// to get all surface information and build a layout tree before it actually
    /// generates thundr surfaces for each node. This allows it to extract all
    /// glyph positions into each LayoutNode, and later use the
    /// get_thundr_surf_for_glyph helper to generate surfaces for them.
    ///
    /// Our Font instance is going to use the provided Thundr context to
    /// create surfaces and lay them out. It's going to update the surface
    /// list with them along the way.
    ///
    /// The cursor argument allows for itemizing runs of different fonts. The
    /// text layout creation will continue at that point.
    pub fn layout_text<F>(
        &mut self,
        display: &mut th::Display,
        cursor: &mut Cursor,
        text: &[CachedChar],
        glyph_callback: &mut F,
    ) where
        F: FnMut(&mut Self, &mut th::Display, &mut Cursor, &CachedChar),
    {
        // For each itemized text run we need to reset the index that
        // the cursor is using, since we will be using a different infos
        // array and we may accidentally use an old size
        cursor.c_i = 0;

        self.for_each_text_block(display, cursor, text, glyph_callback)
    }

    pub fn initialize_cached_chars(
        &mut self,
        display: &mut th::Display,
        inst: &mut ll::Instance,
        glyphs: &mut ll::Snapshot<Glyph>,
        text: &str,
    ) -> Vec<CachedChar> {
        // Set up our HarfBuzz buffers
        let mut buffer = hb::Buffer::new();
        buffer.set_direction(hb::Direction::LTR);
        buffer.add_str(text);
        let mut ret = Vec::new();

        // Now the big call to get the shaping information
        unsafe { hb_sys::hb_shape(self.f_hb_raw_font, buffer.as_ptr(), std::ptr::null(), 0) };
        let infos = unsafe {
            let mut size: u32 = 0;
            let r = hb_sys::hb_buffer_get_glyph_infos(buffer.as_ptr(), &mut size as *mut _);
            std::slice::from_raw_parts(r, size as usize)
        };
        let positions = unsafe {
            let mut size: u32 = 0;
            let r = hb_sys::hb_buffer_get_glyph_positions(buffer.as_ptr(), &mut size as *mut _);
            std::slice::from_raw_parts(r, size as usize)
        };

        for i in 0..infos.len() {
            let raw_glyph_id = infos[i].codepoint as u16;
            self.ensure_glyph_exists(display, inst, glyphs, raw_glyph_id);
            let glyph_id = self.f_glyphs[raw_glyph_id as usize]
                .as_ref()
                .expect("Bug: No Glyph created for this character");
            let glyph = glyphs.get(&glyph_id).unwrap();

            let (x_offset, y_offset, x_advance, y_advance) = scale_hb_positions(&positions[i]);

            ret.push(CachedChar {
                node: inst.add_entity(),
                glyph_id: glyph_id.clone(),
                raw_glyph_id: raw_glyph_id,
                cursor_advance: (x_advance, y_advance),
                offset: (
                    x_offset + glyph.g_bitmap_left,
                    y_offset - glyph.g_bitmap_top,
                ),
            });
        }

        return ret;
    }
}
