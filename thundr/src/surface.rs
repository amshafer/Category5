// A `Surface` represents a region that will be drawn on the target.
// Surfaces have `Image`s bound to them, which will be used for compositing
// the final frame.
//
// Essentially the surface is the geometrical region on the screen, and the image
// contents will be sampled into the surface's rectangle.
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate nix;
extern crate ash;

use super::image::Image;
use super::damage::Damage;
use super::renderer::{Renderer,RecordParams,PushConstants};
use utils::region::Rect;

use ash::version::{DeviceV1_0};
use ash::vk;
use std::rc::Rc;
use std::cell::RefCell;

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
    pub(crate) s_damage: Option<Damage>
}

#[derive(PartialEq,Clone)]
pub struct Surface {
    s_internal: Rc<RefCell<SurfaceInternal>>,
}

impl Surface {
    pub(crate) fn create_surface(x: f32,
                                 y: f32,
                                 width: f32,
                                 height: f32)
                                 -> Surface
    {
        Surface {
            s_internal: Rc::new(RefCell::new(SurfaceInternal {
                s_rect: Rect::new(x, y, width, height),
                s_image: None,
                s_damage: None,
            }))
        }
    }

    /// Attaches an image to this surface, when this surface
    /// is drawn the contents will be sample from `image`
    pub(crate) fn bind_image(&mut self, image: Image) {
        self.s_internal.borrow_mut().s_image = Some(image);
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

impl Renderer {
    /// Generate draw calls for this image
    ///
    /// It is a very common operation to draw a image, this
    /// helper draws itself at the locations passed by `push`
    ///
    /// First all descriptor sets and input assembly is bound
    /// before the call to vkCmdDrawIndexed. The descriptor
    /// sets should be updated whenever window contents are
    /// changed, and then cbufs should be regenerated using this.
    ///
    /// Must be called while recording a cbuf
    pub fn record_surface_draw(&self,
                               params: &RecordParams,
                               thundr_surf: &Surface,
                               depth: f32)
    {
        let surf = thundr_surf.s_internal.borrow();
        let image = match surf.s_image.as_ref() {
            Some(i) => i,
            None => return,
        }.i_internal.borrow();

        let push = PushConstants {
            order: depth,
            x: surf.s_rect.r_pos.0,
            y: surf.s_rect.r_pos.1,
            width: surf.s_rect.r_size.0,
            height: surf.s_rect.r_size.1,
        };

        unsafe {
            if let Some(ctx) = &*self.app_ctx.borrow() {
                // Descriptor sets can be updated elsewhere, but
                // they must be bound before drawing
                //
                // We need to bind both the uniform set, and the per-Image
                // set for the image sampler
                self.dev.cmd_bind_descriptor_sets(
                    params.cbuf,
                    vk::PipelineBindPoint::GRAPHICS,
                    ctx.pipeline_layout,
                    0, // first set
                    &[
                        ctx.ubo_descriptor,
                        image.i_sampler_descriptors[params.image_num],
                    ],
                    &[], // dynamic offsets
                );

                // Set the z-ordering of the window we want to render
                // (this sets the visible window ordering)
                self.dev.cmd_push_constants(
                    params.cbuf,
                    ctx.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0, // offset
                    // get a &[u8] from our struct
                    // TODO: This should go. It is showing up as a noticeable
                    // hit in profiling. Idk if there is a safe way to
                    // replace it.
                    bincode::serialize(&push).unwrap().as_slice(),
                );

                // Here is where everything is actually drawn
                // technically 3 vertices are being drawn
                // by the shader
                self.dev.cmd_draw_indexed(
                    params.cbuf, // drawing command buffer
                    ctx.vert_count, // number of verts
                    1, // number of instances
                    0, // first vertex
                    0, // vertex offset
                    1, // first instance
                );
            }
        }
    }
}
