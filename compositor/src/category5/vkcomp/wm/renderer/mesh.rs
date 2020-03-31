// Meshes represent a textured quad used to draw 2D
// graphics
//
// Austin Shafer - 2020

#![allow(dead_code, non_camel_case_types)]
extern crate ash;

use super::*;
use ash::version::{DeviceV1_0};
use ash::vk;

// A single 3D object, stored in indexed vertex form
//
// All 3D objects should be stored as a set of vertices, which
// are combined into a mesh by selecting indices. This is typical stuff.
//
// meshes are created with Renderer::create_mesh. The renderer is in
// charge of creating/destroying the meshes since all of the mesh
// resources are created from the Renderer.
#[derive(Debug)]
pub struct Mesh {
    // image containing the contents of the window
    pub image: vk::Image,
    pub image_view: vk::ImageView,
    pub image_mem: vk::DeviceMemory,
    pub image_resolution: vk::Extent2D,
    pub pool_handle: usize,
    pub sampler_descriptors: Vec<vk::DescriptorSet>,
}

impl Mesh {
    // A simple teardown function. The renderer is needed since
    // it allocated all these objects.
    pub fn destroy(&self, rend: &Renderer) {
        unsafe {
            rend.dev.destroy_image(self.image, None);
            rend.dev.destroy_image_view(self.image_view, None);
            rend.dev.free_memory(self.image_mem, None);
        }
    }

    // Generate draw calls for this mesh
    //
    // It is a very common operation to draw a mesh, this
    // helper draws itself at the locations passed by `push`
    //
    // First all descriptor sets and input assembly is bound
    // before the call to vkCmdDrawIndexed. The descriptor
    // sets should be updated whenever window contents are
    // changed, and then cbufs should be regenerated using this.
    //
    // Must be called while recording a cbuf
    pub fn record_draw(&self,
                       rend: &Renderer,
                       params: &RecordParams,
                       push: &PushConstants)
    {
        unsafe {
            if let Some(ctx) = &*rend.app_ctx.borrow() {
                // Descriptor sets can be updated elsewhere, but
                // they must be bound before drawing
                //
                // We need to bind both the uniform set, and the per-Mesh
                // set for the image sampler
                rend.dev.cmd_bind_descriptor_sets(
                    params.cbuf,
                    vk::PipelineBindPoint::GRAPHICS,
                    ctx.pipeline_layout,
                    0, // first set
                    &[
                        ctx.ubo_descriptor,
                        self.sampler_descriptors[params.image_num],
                    ],
                    &[], // dynamic offsets
                );

                // Set the z-ordering of the window we want to render
                // (this sets the visible window ordering)
                rend.dev.cmd_push_constants(
                    params.cbuf,
                    ctx.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0, // offset
                    // get a &[u8] from our struct
                    // TODO: This should go. It is showing up as a noticeable
                    // hit in profiling. Idk if there is a safe way to
                    // replace it.
                    bincode::serialize(push).unwrap().as_slice(),
                );

                // Here is where everything is actually drawn
                // technically 3 vertices are being drawn
                // by the shader
                rend.dev.cmd_draw_indexed(
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
