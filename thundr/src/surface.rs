// A `Surface` represents a region that will be drawn on the target.
// Surfaces have `Image`s bound to them, which will be used for compositing
// the final frame.
//
// Essentially the surface is the geometrical region on the screen, and the image
// contents will be sampled into the surface's rectangle.
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate nix;

use super::image::Image;
use utils::region::Rect;

use std::cell::RefCell;
use std::rc::Rc;

/// A surface represents a geometric region that will be
/// drawn. It needs to have an image attached. The same
/// image can be bound to multiple surfaces.
#[derive(PartialEq)]
pub(crate) struct SurfaceInternal {
    /// The position and size of the surface
    pub s_rect: Rect<f32>,
    /// The size of the surface
    /// The currently attached image
    pub(crate) s_image: Option<Image>,
}

#[derive(PartialEq, Clone)]
pub struct Surface {
    pub(crate) s_internal: Rc<RefCell<SurfaceInternal>>,
}

impl Surface {
    pub(crate) fn create_surface(x: f32, y: f32, width: f32, height: f32) -> Surface {
        Surface {
            s_internal: Rc::new(RefCell::new(SurfaceInternal {
                s_rect: Rect::new(x, y, width, height),
                s_image: None,
            })),
        }
    }

    /// Attaches an image to this surface, when this surface
    /// is drawn the contents will be sample from `image`
    pub(crate) fn bind_image(&mut self, image: Image) {
        self.s_internal.borrow_mut().s_image = Some(image);
    }

    pub(crate) fn get_image(&self) -> Option<Image> {
        self.s_internal.borrow().s_image.clone()
    }

    pub fn get_pos(&self) -> (f32, f32) {
        let surf = self.s_internal.borrow();

        (surf.s_rect.r_pos.0, surf.s_rect.r_pos.1)
    }
    pub fn set_pos(&mut self, x: f32, y: f32) {
        let mut surf = self.s_internal.borrow_mut();
        surf.s_rect.r_pos.0 = x;
        surf.s_rect.r_pos.1 = y;
    }

    pub fn get_size(&self) -> (f32, f32) {
        let surf = self.s_internal.borrow();

        (surf.s_rect.r_size.0, surf.s_rect.r_size.1)
    }
    pub fn set_size(&mut self, w: f32, h: f32) {
        let mut surf = self.s_internal.borrow_mut();
        surf.s_rect.r_size.0 = w;
        surf.s_rect.r_size.1 = h;
    }
}
