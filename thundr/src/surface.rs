// A `Surface` represents a region that will be drawn on the target.
// Surfaces have `Image`s bound to them, which will be used for compositing
// the final frame.
//
// Essentially the surface is the geometrical region on the screen, and the image
// contents will be sampled into the surface's rectangle.
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate nix;
use crate::Damage;

use super::image::Image;
use utils::region::Rect;

use std::cell::RefCell;
use std::rc::Rc;

/// A surface represents a geometric region that will be
/// drawn. It needs to have an image attached. The same
/// image can be bound to multiple surfaces.
#[derive(PartialEq)]
pub(crate) struct SurfaceInternal {
    /// The position and size of the surface.
    pub s_rect: Rect<f32>,
    /// The size of the surface.
    /// The currently attached image.
    pub(crate) s_image: Option<Image>,
    /// Damage caused by moving or altering the surface itself.
    s_damage: Option<Damage>,
    /// Was this surface moved/mapped? This signifies if the pipeline needs
    /// to update its data
    pub(crate) s_was_damaged: bool,
    /// A list of subsurfaces.
    /// Surfaces may be layered above one another. This allows us to model wayland
    /// subsurfaces. The surfaces here will be drawn in-order on top of the base
    /// surface.
    pub s_subsurfaces: Vec<Surface>,
}

impl SurfaceInternal {
    /// adjusts from image-coords to surface-coords.
    pub fn get_opaque(&self) -> Option<Rect<i32>> {
        if let Some(image_rc) = self.s_image.as_ref() {
            let image = image_rc.i_internal.borrow();
            if let Some(opaque) = image.i_opaque.as_ref() {
                // We need to scale from the image size to the
                // size of this particular surface
                let scale = (
                    image.i_image_resolution.width as f32 / self.s_rect.r_size.0,
                    image.i_image_resolution.height as f32 / self.s_rect.r_size.1,
                );

                return Some(Rect::new(
                    (opaque.r_pos.0 as f32 / scale.0) as i32,
                    (opaque.r_pos.1 as f32 / scale.1) as i32,
                    (opaque.r_size.0 as f32 / scale.0) as i32,
                    (opaque.r_size.1 as f32 / scale.1) as i32,
                ));
            }
        }
        return None;
    }

    fn record_damage(&mut self) {
        self.s_was_damaged = true;
        let new_rect = self.s_rect.into();

        if let Some(d) = self.s_damage.as_mut() {
            d.add(&new_rect);
        } else {
            self.s_damage = Some(Damage::new(vec![new_rect]));
        }
    }
}

/// A surface that describes how an `Image` should be displayed onscreen
///
/// Surfaces are placed into `SurfaceLists`, which are proccessed and rendered
/// by Thundr. A surface should only ever be used with one `SurfaceList`. If you would
/// like to show the same image on multiple lists, then create multiple surfaces per-list.
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
                s_damage: None,
                s_was_damaged: false,
                s_subsurfaces: Vec::with_capacity(0), // this keeps us from allocating
            })),
        }
    }

    /// Internally record a damage rectangle for the dimensions
    /// of this surface.
    ///
    /// Methods that alter the surface should be wrapped in two
    /// calls to this to record their movement.
    pub(crate) fn record_damage(&mut self) {
        self.s_internal.borrow_mut().record_damage();
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
        if surf.s_rect.r_pos.0 != x || surf.s_rect.r_pos.1 != y {
            surf.record_damage();
            surf.s_rect.r_pos.0 = x;
            surf.s_rect.r_pos.1 = y;
            surf.record_damage();
        }
    }

    pub fn get_size(&self) -> (f32, f32) {
        let surf = self.s_internal.borrow();

        (surf.s_rect.r_size.0, surf.s_rect.r_size.1)
    }

    pub fn set_size(&mut self, w: f32, h: f32) {
        let mut surf = self.s_internal.borrow_mut();
        if surf.s_rect.r_size.0 != w || surf.s_rect.r_size.1 != h {
            surf.record_damage();
            surf.s_rect.r_size.0 = w;
            surf.s_rect.r_size.1 = h;
            surf.record_damage();
        }
    }

    /// adjusts from image-coords to surface-coords.
    pub fn get_opaque(&self) -> Option<Rect<i32>> {
        let surf = self.s_internal.borrow();
        return surf.get_opaque();
    }

    /// adjusts damage from image-coords to surface-coords.
    pub fn get_damage(&self) -> Option<Damage> {
        let surf = self.s_internal.borrow();
        if let Some(image_rc) = surf.s_image.as_ref() {
            let image = image_rc.i_internal.borrow();
            if let Some(damage) = image.i_damage.as_ref() {
                let mut ret = Damage::empty();
                // We need to scale the damage from the image size to the
                // size of this particular surface
                let scale = (
                    image.i_image_resolution.width as f32 / surf.s_rect.r_size.0,
                    image.i_image_resolution.height as f32 / surf.s_rect.r_size.1,
                );

                for r in damage.regions() {
                    ret.add(&Rect::new(
                        (r.r_pos.0 as f32 / scale.0) as i32,
                        (r.r_pos.1 as f32 / scale.1) as i32,
                        (r.r_size.0 as f32 / scale.0) as i32,
                        (r.r_size.1 as f32 / scale.1) as i32,
                    ));
                }
                return Some(ret);
            }
        }
        return None;
    }

    pub(crate) fn take_surface_damage(&self) -> Option<Damage> {
        self.s_internal.borrow_mut().s_damage.take()
    }

    /// This appends `surf` to the end of the subsurface list
    pub fn add_subsurface(&mut self, surf: Surface) {
        self.s_internal.borrow_mut().s_subsurfaces.push(surf);
    }

    pub fn get_subsurface(&self, i: usize) -> Surface {
        let internal = self.s_internal.borrow();
        assert!(internal.s_subsurfaces.len() > i);

        internal.s_subsurfaces[i].clone()
    }
}
