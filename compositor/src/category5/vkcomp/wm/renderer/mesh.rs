// Meshes represent a textured quad used to draw 2D
// graphics
//
// Austin Shafer - 2020

#![allow(dead_code, non_camel_case_types)]
extern crate ash;

use crate::category5::utils::*;
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
    // specific to the type of image
    m_priv: MeshPrivate,
}

#[derive(Debug)]
enum MeshPrivate {
    mem_image(MemImagePrivate),
}

// Private data for shm images
#[derive(Debug)]
struct MemImagePrivate {
    // The staging buffer for copies to mesh.image
    transfer_buf: vk::Buffer,
    transfer_mem: vk::DeviceMemory,
}

impl Mesh {
    // Create a mesh and its needed data
    //
    // All resources will be allocated by
    // rend
    pub fn new(rend: &mut Renderer,
               texture: WindowContents)
               -> Option<Mesh>
    {
        match texture {
            WindowContents::mem_image(m) =>
                Mesh::from_mem_image(rend, m),
            WindowContents::dmabuf(d) =>
                Mesh::from_dmabuf(rend, d),
        }
    }

    fn from_mem_image(rend: &mut Renderer,
                      img: &MemImage)
                      -> Option<Mesh>
    {
        unsafe {
            let tex_res = vk::Extent2D {
                width: img.width as u32,
                height: img.height as u32,
            };

            // TODO: make this cached in Renderer
            let mem_props = Renderer::get_pdev_mem_properties(&rend.inst,
                                                              rend.pdev);

            // The image is created with DEVICE_LOCAL memory types,
            // so we need to make a staging buffer to copy the data from.
            let (buffer, buf_mem) = rend.create_buffer(
                vk::BufferUsageFlags::TRANSFER_SRC,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                img.as_slice(),
            );

            // This image will back the contents of the on-screen
            // client window.
            // TODO: this should eventually just use the image reported from
            // wayland.
            let (image, view, img_mem) = rend.create_image_with_contents(
                &mem_props,
                &tex_res,
                vk::Format::R8G8B8A8_SRGB,
                vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_DST,
                vk::ImageAspectFlags::COLOR,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                buffer,
            );

            if let Some(ctx) = &mut *rend.app_ctx.borrow_mut() {
                // each mesh holds a set of descriptors that it will
                // bind before drawing itself. This set holds the
                // image sampler.
                //
                // right now they only hold an image sampler
                let (handle, descriptors) = ctx.desc_pool.allocate_samplers(
                    &rend,
                    rend.fb_count,
                );

                for i in 0..rend.fb_count {
                    // bind the texture for our window
                    rend.update_sampler_descriptor_set(
                        descriptors[i],
                        1, //n binding
                        0, // element
                        ctx.image_sampler,
                        view,
                    );
                }

                return Some(Mesh {
                    image: image,
                    image_view: view,
                    image_mem: img_mem,
                    image_resolution: tex_res,
                    pool_handle: handle,
                    sampler_descriptors: descriptors,
                    m_priv: MeshPrivate::mem_image(
                        MemImagePrivate {
                            transfer_buf: buffer,
                            transfer_mem: buf_mem,
                        }
                    ),
                });
            }
            return None;
        }
    }

    fn from_dmabuf(rend: &Renderer,
                   dmabuf: &Dmabuf)
                   -> Option<Mesh>
    {
        unsafe {
            return None;
        }
    }

    // Create a mesh and its needed data
    //
    // All resources will be allocated by
    // rend
    pub fn update_contents(&mut self,
                           rend: &mut Renderer,
                           data: WindowContents)
    {
        match data {
            WindowContents::mem_image(m) =>
                self.update_from_mem_image(rend, m),
            WindowContents::dmabuf(d) => {},
        };
    }

    fn update_from_mem_image(&mut self,
                             rend: &mut Renderer,
                             img: &MemImage)
    {
        if let MeshPrivate::mem_image(m) = &self.m_priv {
            unsafe {
                // copy the data into the staging buffer
                rend.update_memory(m.transfer_mem,
                                   img.as_slice());
                // copy the staging buffer into the image
                rend.update_image_contents_from_buf(
                    m.transfer_buf,
                    self.image,
                    self.image_resolution.width,
                    self.image_resolution.height,
                );
            }
        }
    }

    // A simple teardown function. The renderer is needed since
    // it allocated all these objects.
    pub fn destroy(&self, rend: &Renderer) {
        unsafe {
            rend.dev.destroy_image(self.image, None);
            rend.dev.destroy_image_view(self.image_view, None);
            rend.dev.free_memory(self.image_mem, None);
            match &self.m_priv {
                MeshPrivate::mem_image(m) => {
                    rend.dev.destroy_buffer(m.transfer_buf, None);
                    rend.dev.free_memory(m.transfer_mem, None);
                },
                //MeshPrivate::dmabuf(d) => {},
            }
            // get the descriptor pool
            if let Some(ctx) = &mut *rend.app_ctx.borrow_mut() {
                // free our descriptors
                ctx.desc_pool.destroy_samplers(rend,
                                               self.pool_handle,
                                               self.sampler_descriptors
                                               .as_slice());
            }
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
