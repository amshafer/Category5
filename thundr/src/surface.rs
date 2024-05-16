// A `Surface` represents a region that will be drawn on the target.
// Surfaces have `Image`s bound to them, which will be used for compositing
// the final frame.
//
// Essentially the surface is the geometrical region on the screen, and the image
// contents will be sampled into the surface's rectangle.
// Austin Shafer - 2020
extern crate nix;

use super::image::Image;
use utils::region::Rect;

/// A surface represents a geometric region that will be
/// drawn. It needs to have an image attached. The same
/// image can be bound to multiple surfaces.
#[derive(PartialEq, Debug, Default)]
pub struct Surface {
    /// The position and size of the surface.
    pub s_rect: Rect<i32>,
    /// The size of the surface.
    /// The currently attached image.
    pub s_image: Option<Image>,
    /// For rendering a surface as a constant color
    /// This is mutually exclusive to s_image
    pub s_color: Option<(f32, f32, f32, f32)>,
}

impl Surface {
    pub fn new(
        geometry: Rect<i32>,
        image: Option<Image>,
        color: Option<(f32, f32, f32, f32)>,
    ) -> Self {
        Self {
            s_rect: geometry,
            s_image: image,
            s_color: color,
        }
    }

    pub fn bind_image(&mut self, image: Image) {
        self.s_image = Some(image);
    }

    pub fn get_image(&self) -> Option<Image> {
        self.s_image.clone()
    }

    pub fn get_pos(&self) -> (i32, i32) {
        (self.s_rect.r_pos.0, self.s_rect.r_pos.1)
    }
    pub fn set_pos(&mut self, x: i32, y: i32) {
        if self.s_rect.r_pos.0 != x || self.s_rect.r_pos.1 != y {
            self.s_rect.r_pos.0 = x;
            self.s_rect.r_pos.1 = y;
        }
    }

    pub fn get_size(&self) -> (i32, i32) {
        (self.s_rect.r_size.0, self.s_rect.r_size.1)
    }

    pub fn set_size(&mut self, w: i32, h: i32) {
        if self.s_rect.r_size.0 != w || self.s_rect.r_size.1 != h {
            self.s_rect.r_size.0 = w;
            self.s_rect.r_size.1 = h;
        }
    }

    pub fn get_color(&self) -> Option<(f32, f32, f32, f32)> {
        self.s_color
    }

    pub fn set_color(&mut self, color: (f32, f32, f32, f32)) {
        self.s_color = Some(color);
    }
}
