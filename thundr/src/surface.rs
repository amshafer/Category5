// A `Surface` represents a region that will be drawn on the target.
// Surfaces have `Image`s bound to them, which will be used for compositing
// the final frame.
//
// Essentially the surface is the geometrical region on the screen, and the image
// contents will be sampled into the surface's rectangle.
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate nix;
use crate::{Device, Result, ThundrError};
use lluvia as ll;

use super::image::Image;
use utils::{log, region::Rect};

use std::sync::{Arc, RwLock};

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

impl Drop for SurfaceInternal {
    fn drop(&mut self) {
        // Remove all references to this in the subsurface's parent fields
        for surf in self.s_subsurfaces.iter() {
            surf.s_internal.write().unwrap().s_parent = None;
        }
    }
}

impl SurfaceInternal {
    fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            s_rect: Rect::new(x, y, width, height),
            s_image: None,
            s_color: None,
            s_subsurfaces: Vec::with_capacity(0), // this keeps us from allocating
            s_parent: None,
            // When the surface is first created we don't send its size to
            // the GPU. We mark it as modified and the first time it is used
            // it will be flushed
            s_modified: true,
        }
    }

    /// adjusts from image-coords to surface-coords.
    pub fn get_opaque(&self, dev: &Arc<Device>) -> Option<Rect<i32>> {
        if let Some(image_rc) = self.s_image.as_ref() {
            let image = image_rc.i_internal.read().unwrap();
            if let Some(opaque) = image.i_opaque.as_ref() {
                let image_vk = dev.d_image_vk.get(&image.i_id).unwrap();
                // We need to scale from the image size to the
                // size of this particular surface
                let scale = (
                    image_vk.iv_image_resolution.width as f32 / self.s_rect.r_size.0,
                    image_vk.iv_image_resolution.height as f32 / self.s_rect.r_size.1,
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
    pub(crate) s_internal: Arc<RwLock<SurfaceInternal>>,
}

impl PartialEq for Surface {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.s_internal, &other.s_internal)
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
            s_window_id: id.clone(),
            s_internal: Arc::new(RwLock::new(SurfaceInternal::new(x, y, width, height))),
        }
    }

    /// Remove all state from this surface, resetting it to the specified parameters
    ///
    /// This does not remove this surface from its parent
    pub fn reset_surface(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let mut internal = self.s_internal.write().unwrap();
        *internal = SurfaceInternal::new(x, y, width, height);
    }

    /// Get the ECS Id tracking this resource
    pub fn get_ecs_id(&self) -> ll::Entity {
        self.s_window_id.clone()
    }

    /// Does this surface need to have its size and offset flushed to the
    /// GPU's window list?
    pub fn modified(&self) -> bool {
        self.s_internal.read().unwrap().s_modified
    }

    pub fn set_modified(&mut self, modified: bool) {
        self.s_internal.write().unwrap().s_modified = modified;
    }

    /// Attaches an image to this surface, when this surface
    /// is drawn the contents will be sample from `image`
    pub fn bind_image(&mut self, image: Image) {
        let mut surf = self.s_internal.write().unwrap();
        assert!(surf.s_color.is_none());
        surf.s_modified = true;
        surf.s_image = Some(image);
    }

    pub fn get_image(&self) -> Option<Image> {
        self.s_internal.read().unwrap().s_image.clone()
    }

    pub fn get_pos(&self) -> (f32, f32) {
        let surf = self.s_internal.read().unwrap();

        (surf.s_rect.r_pos.0, surf.s_rect.r_pos.1)
    }
    pub fn set_pos(&mut self, x: f32, y: f32) {
        self.set_modified(true);
        let mut surf = self.s_internal.write().unwrap();
        if surf.s_rect.r_pos.0 != x || surf.s_rect.r_pos.1 != y {
            surf.s_rect.r_pos.0 = x;
            surf.s_rect.r_pos.1 = y;
        }
    }

    pub fn get_size(&self) -> (f32, f32) {
        let surf = self.s_internal.read().unwrap();

        (surf.s_rect.r_size.0, surf.s_rect.r_size.1)
    }

    pub fn set_size(&mut self, w: f32, h: f32) {
        self.set_modified(true);
        let mut surf = self.s_internal.write().unwrap();
        if surf.s_rect.r_size.0 != w || surf.s_rect.r_size.1 != h {
            surf.s_rect.r_size.0 = w;
            surf.s_rect.r_size.1 = h;
        }
    }

    pub fn get_color(&self) -> Option<(f32, f32, f32, f32)> {
        let surf = self.s_internal.read().unwrap();
        surf.s_color
    }

    pub fn set_color(&mut self, color: (f32, f32, f32, f32)) {
        self.set_modified(true);
        let mut surf = self.s_internal.write().unwrap();
        surf.s_color = Some(color);
    }

    pub fn get_opaque(&self, dev: &Arc<Device>) -> Option<Rect<i32>> {
        let surf = self.s_internal.read().unwrap();
        return surf.get_opaque(dev);
    }

    /// This appends `surf` to the end of the subsurface list
    pub fn add_subsurface(&mut self, surf: Surface) {
        {
            let mut internal = surf.s_internal.write().unwrap();
            // only one parent may be set at a time
            assert!(internal.s_parent.is_none());
            internal.s_parent = Some(self.clone());
        }

        // Push since we are reverse order
        self.s_internal.write().unwrap().s_subsurfaces.push(surf);
    }

    /// This appends `surf` to the end of the subsurface list
    pub fn remove_subsurface(&mut self, surf: Surface) -> Result<()> {
        {
            let mut surf_internal = surf.s_internal.write().unwrap();
            // make sure this is a subsurface
            if surf_internal.s_parent.as_ref().unwrap().clone() != *self {
                log::debug!("Cannot remove subsurface because we are not its parent");
                return Err(ThundrError::SURFACE_NOT_FOUND);
            }
            surf_internal.s_parent = None;
        }

        let mut internal = self.s_internal.write().unwrap();
        let pos = internal
            .s_subsurfaces
            .iter()
            .position(|s| *s == surf)
            .ok_or(ThundrError::SURFACE_NOT_FOUND)?;
        internal.s_subsurfaces.remove(pos);
        Ok(())
    }

    /// Recursively unbind all subsurfaces in this surface tree
    pub fn remove_all_subsurfaces(&mut self) {
        let mut internal = self.s_internal.write().unwrap();

        for surf in internal.s_subsurfaces.iter_mut() {
            surf.remove_all_subsurfaces();
        }

        internal.s_subsurfaces.clear();
        internal.s_parent = None;
    }

    pub fn get_parent(&self) -> Option<Surface> {
        self.s_internal
            .write()
            .unwrap()
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
        let mut internal = self.s_internal.write().unwrap();
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
        self.s_internal.read().unwrap().s_subsurfaces.len()
    }

    pub fn get_subsurface(&self, i: usize) -> Surface {
        let internal = self.s_internal.read().unwrap();
        assert!(internal.s_subsurfaces.len() > i);

        internal.s_subsurfaces[i].clone()
    }
}

pub enum SubsurfaceOrder {
    Above,
    Below,
}
