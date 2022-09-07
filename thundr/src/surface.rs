// A `Surface` represents a region that will be drawn on the target.
// Surfaces have `Image`s bound to them, which will be used for compositing
// the final frame.
//
// Essentially the surface is the geometrical region on the screen, and the image
// contents will be sampled into the surface's rectangle.
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate nix;
use crate::{Damage, Result, ThundrError};
use lluvia as ll;

use super::image::Image;
use utils::{log, region::Rect};

use std::cell::RefCell;
use std::rc::Rc;

/// A surface represents a geometric region that will be
/// drawn. It needs to have an image attached. The same
/// image can be bound to multiple surfaces.
#[derive(PartialEq, Debug, Default)]
pub(crate) struct SurfaceInternal {
    /// The position and size of the surface.
    pub s_rect: Rect<f32>,
    /// The size of the surface.
    /// The currently attached image.
    pub(crate) s_image: Option<Image>,
    /// For rendering a surface as a constant color
    /// This is mutually exclusive to s_image
    pub(crate) s_color: Option<(f32, f32, f32, f32)>,
    /// Damage caused by moving or altering the surface itself.
    s_damage: Option<Damage>,
    /// This is the surface damage that has been attached by clients.
    /// It differs from s_damage in that it needs to be offset by the surface pos.
    s_surf_damage: Option<Damage>,
    /// Was this surface moved/mapped? This signifies if the pipeline needs
    /// to update its data
    pub(crate) s_was_damaged: bool,
    /// A list of subsurfaces.
    /// Surfaces may be layered above one another. This allows us to model wayland
    /// subsurfaces. The surfaces here will be drawn in-order on top of the base
    /// surface.
    ///
    /// This list is reversed of what you think the order is. The "front" subsurface
    /// is really at the back of the list. This prevents us from having to shift
    /// so many elements when doing switches.
    pub s_subsurfaces: Vec<Surface>,
    /// If this is a subsurface, this will point to the parent we need to
    /// remove it from.
    pub s_parent: Option<Surface>,
    /// Does this surface need to flush its contents to the window list?
    s_modified: bool,
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
        self.s_modified = true;
        self.s_was_damaged = true;
        let new_rect = self.s_rect.into();

        if let Some(d) = self.s_damage.as_mut() {
            d.add(&new_rect);
        } else {
            self.s_damage = Some(Damage::new(vec![new_rect]));
        }
    }

    fn damage(&mut self, other: Damage) {
        self.s_surf_damage = Some(other);
    }
}

/// A surface that describes how an `Image` should be displayed onscreen
///
/// Surfaces are placed into `SurfaceLists`, which are proccessed and rendered
/// by Thundr. A surface should only ever be used with one `SurfaceList`. If you would
/// like to show the same image on multiple lists, then create multiple surfaces per-list.
#[derive(Debug, Clone)]
pub struct Surface {
    /// The Thundr window list Id. This is an ECS ID to track surface updates
    /// in the shader's list.
    pub s_window_id: ll::Entity,
    pub(crate) s_internal: Rc<RefCell<SurfaceInternal>>,
}

impl PartialEq for Surface {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.s_internal, &other.s_internal)
    }
}

impl Surface {
    pub(crate) fn create_surface(
        id: ll::Entity,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Surface {
        Surface {
            s_window_id: id,
            s_internal: Rc::new(RefCell::new(SurfaceInternal {
                s_rect: Rect::new(x, y, width, height),
                s_image: None,
                s_color: None,
                s_damage: None,
                s_surf_damage: None,
                s_was_damaged: false,
                s_subsurfaces: Vec::with_capacity(0), // this keeps us from allocating
                s_parent: None,
                // When the surface is first created we don't send its size to
                // the GPU. We mark it as modified and the first time it is used
                // it will be flushed
                s_modified: true,
            })),
        }
    }

    /// Get the ECS Id tracking this resource
    pub fn get_ecs_id(&self) -> ll::Entity {
        self.s_window_id.clone()
    }

    /// Does this surface need to have its size and offset flushed to the
    /// GPU's window list?
    pub fn modified(&self) -> bool {
        self.s_internal.borrow().s_modified
    }

    pub fn set_modified(&mut self, modified: bool) {
        self.s_internal.borrow_mut().s_modified = modified;
    }

    /// Internally record a damage rectangle for the dimensions
    /// of this surface.
    ///
    /// Methods that alter the surface should be wrapped in two
    /// calls to this to record their movement.
    pub(crate) fn record_damage(&mut self) {
        self.s_internal.borrow_mut().record_damage();
    }

    /// Thundr clients use this to add *surface* damage.
    pub fn damage(&mut self, other: Damage) {
        self.s_internal.borrow_mut().damage(other);
    }

    /// Attaches an image to this surface, when this surface
    /// is drawn the contents will be sample from `image`
    pub(crate) fn bind_image(&mut self, image: Image) {
        let mut surf = self.s_internal.borrow_mut();
        assert!(surf.s_color.is_none());
        surf.s_image = Some(image);
    }

    pub fn get_image(&self) -> Option<Image> {
        self.s_internal.borrow().s_image.clone()
    }

    pub fn get_pos(&self) -> (f32, f32) {
        let surf = self.s_internal.borrow();

        (surf.s_rect.r_pos.0, surf.s_rect.r_pos.1)
    }
    pub fn set_pos(&mut self, x: f32, y: f32) {
        self.set_modified(true);
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
        self.set_modified(true);
        let mut surf = self.s_internal.borrow_mut();
        if surf.s_rect.r_size.0 != w || surf.s_rect.r_size.1 != h {
            surf.record_damage();
            surf.s_rect.r_size.0 = w;
            surf.s_rect.r_size.1 = h;
            surf.record_damage();
        }
    }

    pub fn get_color(&self) -> Option<(f32, f32, f32, f32)> {
        let surf = self.s_internal.borrow();
        surf.s_color
    }

    pub fn set_color(&mut self, color: (f32, f32, f32, f32)) {
        self.set_modified(true);
        let mut surf = self.s_internal.borrow_mut();
        assert!(surf.s_image.is_none());
        surf.s_color = Some(color);
    }

    pub fn get_opaque(&self) -> Option<Rect<i32>> {
        let surf = self.s_internal.borrow();
        return surf.get_opaque();
    }

    /// Get's damage. Returned values are in surface coordinates.
    pub fn get_surf_damage(&mut self) -> Option<Damage> {
        let mut surf = self.s_internal.borrow_mut();
        let mut ret = Damage::empty();
        let surf_extent = Rect::new(
            0,
            0,
            surf.s_rect.r_size.0 as i32,
            surf.s_rect.r_size.1 as i32,
        );

        // First add up the damage from the buffer
        if let Some(image_rc) = surf.s_image.as_ref() {
            let image = image_rc.i_internal.borrow();
            if let Some(damage) = image.i_damage.as_ref() {
                // We need to scale the damage from the image size to the
                // size of this particular surface
                let scale = (
                    image.i_image_resolution.width as f32 / surf.s_rect.r_size.0,
                    image.i_image_resolution.height as f32 / surf.s_rect.r_size.1,
                );

                for r in damage.regions() {
                    // The image region scaled to surface space
                    let region = Rect::new(
                        (r.r_pos.0 as f32 / scale.0) as i32,
                        (r.r_pos.1 as f32 / scale.1) as i32,
                        (r.r_size.0 as f32 / scale.0) as i32,
                        (r.r_size.1 as f32 / scale.1) as i32,
                    );

                    // Here we scale the image damage onto the surface size, and then
                    // clip it to the max surf extent.
                    ret.add(&region.clip(&surf_extent));
                }
            }
        }

        // Now add in the surface damage
        if let Some(damage) = surf.s_surf_damage.take() {
            for r in damage.regions() {
                // The image region scaled to surface space
                let region = Rect::new(
                    r.r_pos.0 as i32,
                    r.r_pos.1 as i32,
                    r.r_size.0 as i32,
                    r.r_size.1 as i32,
                );
                ret.add(&region.clip(&surf_extent));
            }
        }

        if ret.is_empty() {
            return None;
        }
        return Some(ret);
    }

    /// This gets the surface damage and offsets it into the
    /// screen coordinate space.
    pub fn get_global_damage(&mut self) -> Option<Damage> {
        let mut ret = self.get_surf_damage();
        let surf = self.s_internal.borrow_mut();

        if let Some(surf_damage) = ret.as_mut() {
            for r in surf_damage.d_regions.iter_mut() {
                r.r_pos.0 += surf.s_rect.r_pos.0 as i32;
                r.r_pos.1 += surf.s_rect.r_pos.1 as i32;
            }
        }
        return ret;
    }

    /// This gets damage in image-coords.
    ///
    /// This is used for getting the total amount of damage that the image should be
    /// updated by. It's a union of the unchanged image damage and the screen
    /// damage mapped on the image dimensions.
    pub fn get_image_damage(&mut self) -> Option<Damage> {
        let mut surf = self.s_internal.borrow_mut();
        let surf_damage = surf.s_surf_damage.take();
        let mut ret = Damage::empty();

        // First add up the damage from the buffer
        if let Some(image_rc) = surf.s_image.as_ref() {
            let image = image_rc.i_internal.borrow();
            let image_extent = Rect::new(
                0,
                0,
                image.i_image_resolution.width as i32,
                image.i_image_resolution.height as i32,
            );

            // We need to scale the damage from the image size to the
            // size of this particular surface
            let scale = (
                surf.s_rect.r_size.0 / image.i_image_resolution.width as f32,
                surf.s_rect.r_size.1 / image.i_image_resolution.height as f32,
            );

            if let Some(damage) = image.i_damage.as_ref() {
                ret.union(damage);
            }

            // Now add in the surface damage
            if let Some(damage) = surf_damage {
                for r in damage.regions() {
                    // Remap the damage in image-coords, and clamp at the image size
                    let region = Rect::new(
                        r.r_pos.0,
                        r.r_pos.1,
                        (r.r_size.0 as f32 / scale.0) as i32,
                        (r.r_size.1 as f32 / scale.0) as i32,
                    );

                    ret.add(&region.clip(&image_extent));
                }
            }
        }

        if ret.is_empty() {
            return None;
        }
        return Some(ret);
    }
    pub(crate) fn take_surface_damage(&self) -> Option<Damage> {
        self.s_internal.borrow_mut().s_damage.take()
    }

    /// This appends `surf` to the end of the subsurface list
    pub fn add_subsurface(&mut self, surf: Surface) {
        {
            let mut internal = surf.s_internal.borrow_mut();
            // only one parent may be set at a time
            assert!(internal.s_parent.is_none());
            internal.s_parent = Some(self.clone());
        }

        // Push since we are reverse order
        self.s_internal.borrow_mut().s_subsurfaces.push(surf);
    }

    /// This appends `surf` to the end of the subsurface list
    pub fn remove_subsurface(&mut self, surf: Surface) -> Result<()> {
        {
            let mut surf_internal = surf.s_internal.borrow_mut();
            // make sure this is a subsurface
            if surf_internal.s_parent.as_ref().unwrap().clone() != *self {
                log::debug!("Cannot remove subsurface because we are not its parent");
                return Err(ThundrError::SURFACE_NOT_FOUND);
            }
            surf_internal.s_parent = None;
        }

        let mut internal = self.s_internal.borrow_mut();
        let pos = internal
            .s_subsurfaces
            .iter()
            .position(|s| *s == surf)
            .ok_or(ThundrError::SURFACE_NOT_FOUND)?;
        internal.s_subsurfaces.remove(pos);
        Ok(())
    }

    pub fn get_parent(&self) -> Option<Surface> {
        self.s_internal
            .borrow()
            .s_parent
            .as_ref()
            .map(|p| p.clone())
    }

    /// Move subsurface in front or behind of the other
    pub fn reorder_subsurface(
        &mut self,
        order: SubsurfaceOrder,
        surf: Surface,
        other: Surface,
    ) -> Result<()> {
        let mut internal = self.s_internal.borrow_mut();
        // The index of other within the subsurf list
        let pos = internal
            .s_subsurfaces
            .iter()
            .position(|s| *s == surf)
            .ok_or(ThundrError::SURFACE_NOT_FOUND)?;
        let other_pos = internal
            .s_subsurfaces
            .iter()
            .position(|s| *s == other)
            .ok_or(ThundrError::SURFACE_NOT_FOUND)?;

        internal.s_subsurfaces.remove(pos);
        internal.s_subsurfaces.insert(
            match order {
                SubsurfaceOrder::Above => other_pos,
                SubsurfaceOrder::Below => other_pos + 1,
            },
            surf,
        );

        Ok(())
    }

    pub fn get_subsurface_count(&self) -> usize {
        self.s_internal.borrow().s_subsurfaces.len()
    }

    pub fn get_subsurface(&self, i: usize) -> Surface {
        let internal = self.s_internal.borrow();
        assert!(internal.s_subsurfaces.len() > i);

        internal.s_subsurfaces[i].clone()
    }
}

pub enum SubsurfaceOrder {
    Above,
    Below,
}
