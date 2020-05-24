// Meshes represent a textured quad used to draw 2D
// graphics
//
// Austin Shafer - 2020
#![allow(dead_code, non_camel_case_types)]
extern crate nix;
extern crate ash;

use crate::category5::utils::*;
use super::*;

use nix::unistd::dup;
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
    dmabuf(DmabufPrivate),
    mem_image(MemImagePrivate),
}

// Private data for shm images
#[derive(Debug)]
struct MemImagePrivate {
    // The staging buffer for copies to mesh.image
    transfer_buf: vk::Buffer,
    transfer_mem: vk::DeviceMemory,
}

// Private data for gpu buffers
#[derive(Debug)]
struct DmabufPrivate {
    // we need to cache the params to import memory with
    //
    // memory reqs for the mesh image
    dp_mem_reqs: vk::MemoryRequirements,
    // the type of memory to use
    dp_memtype_index: u32,
    // Stuff to release when we are no longer using
    // this gpu buffer (release the wl_buffer)
    dp_release_info: ReleaseInfo,
}

impl Mesh {
    // Create a mesh and its needed data
    //
    // All resources will be allocated by
    // rend
    pub fn new(rend: &mut Renderer,
               texture: WindowContents,
               release: ReleaseInfo)
               -> Option<Mesh>
    {
        match texture {
            WindowContents::mem_image(m) =>
                Mesh::from_mem_image(rend, m),
            WindowContents::dmabuf(d) =>
                Mesh::from_dmabuf(rend, d, release),
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

            return Mesh::create_common(rend,
                                       MeshPrivate::mem_image(
                                           MemImagePrivate {
                                               transfer_buf: buffer,
                                               transfer_mem: buf_mem,
                                           }),
                                       &tex_res,
                                       image,
                                       img_mem,
                                       view,);
        }
    }

    // returns the index of the memory type to use
    // similar to Renderer::find_memory_type_index
    fn find_memtype_for_dmabuf(dmabuf_type_bits: u32,
                               props: &vk::PhysicalDeviceMemoryProperties,
                               reqs: &vk::MemoryRequirements)
                               -> Option<u32>
    {
        // and find the first type which matches our image
        for (i, ref mem_type) in props.memory_types.iter().enumerate() {
            // Bit i of memoryBitTypes will be set if the resource supports
            // the ith memory type in props.
            //
            // if this index is supported by dmabuf
            if (dmabuf_type_bits >> i) & 1 == 1
                // and by the image
                && (reqs.memory_type_bits >> i) & 1 == 1
                // make sure it is device local
                &&  mem_type.property_flags
                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
            {
                return Some(i as u32);
            }
        }

        return None;
    }

    fn from_dmabuf(rend: &mut Renderer,
                   dmabuf: &Dmabuf,
                   release: ReleaseInfo)
                   -> Option<Mesh>
    {
        println!("Creating mesh from dmabuf {:?}", dmabuf);
        // A lot of this is duplicated from Renderer::create_image
        unsafe {
            // we create the image now, but will have to bind
            // some memory to it later.
            let image_info = vk::ImageCreateInfo::builder()
                .image_type(vk::ImageType::TYPE_2D)
                // TODO: add other formats
                .format(vk::Format::R8G8B8A8_SRGB)
                .extent(vk::Extent3D {
                    width: dmabuf.db_width as u32,
                    height: dmabuf.db_height as u32,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                // we are only doing the linear format for now
                .tiling(vk::ImageTiling::LINEAR)
                .usage(vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let image = rend.dev.create_image(&image_info, None).unwrap();

            // we need to find a memory type that matches the type our
            // new image needs
            let mem_reqs = rend.dev.get_image_memory_requirements(image);
            let mem_props = Renderer::get_pdev_mem_properties(&rend.inst,
                                                              rend.pdev);
            // supported types we can import as
            let dmabuf_type_bits = rend.external_mem_fd_loader
                .get_memory_fd_properties_khr(
                    vk::ExternalMemoryHandleTypeFlags
                        ::EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF,
                    dmabuf.db_fd)
                .expect("Could not get memory fd properties")
                // bitmask set for each supported memory type
                .memory_type_bits;

            let memtype_index = Mesh::find_memtype_for_dmabuf(
                dmabuf_type_bits,
                &mem_props,
                &mem_reqs,
            ).expect("Could not find a memtype for the dmabuf");

            // use some of these to verify dmabuf imports:
            //
            // VkPhysicalDeviceExternalBufferInfo
            // VkPhysicalDeviceExternalImageInfo

            // This is where we differ from create_image
            //
            // We need to import from the dmabuf fd, so we will
            // add a VkImportMemoryFdInfoKHR struct to the next ptr
            // here to tell vulkan that we should import mem
            // instead of allocating it.
            let mut alloc_info = vk::MemoryAllocateInfo::builder()
                .allocation_size(mem_reqs.size)
                .memory_type_index(memtype_index);

            alloc_info.p_next = &vk::ImportMemoryFdInfoKHR::builder()
                .handle_type(vk::ExternalMemoryHandleTypeFlags
                             ::EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF)
                // need to dup the fd since it seems the implementation will
                // internally free it
                .fd(dup(dmabuf.db_fd).unwrap())
                as *const _ as *const std::ffi::c_void;

            // perform the import
            let image_memory = rend.dev.allocate_memory(&alloc_info, None)
                .unwrap();
            rend.dev.bind_image_memory(image, image_memory, 0)
                .expect("Unable to bind device memory to image");

            // finally make a view to wrap the image
            let view_info = vk::ImageViewCreateInfo::builder()
                .subresource_range(
                    vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .level_count(1)
                        .layer_count(1)
                        .build()
                )
                .image(image)
                .format(image_info.format)
                .view_type(vk::ImageViewType::TYPE_2D);

            let view = rend.dev.create_image_view(&view_info, None).unwrap();

            return Mesh::create_common(rend,
                                       MeshPrivate::dmabuf(DmabufPrivate {
                                           dp_mem_reqs: mem_reqs,
                                           dp_memtype_index: memtype_index,
                                           dp_release_info: release,
                                       }),
                                       &vk::Extent2D {
                                           width: dmabuf.db_width as u32,
                                           height: dmabuf.db_height as u32,
                                       },
                                       image,
                                       image_memory,
                                       view);
        }
    }

    fn create_common(rend: &mut Renderer,
                     private: MeshPrivate,
                     res: &vk::Extent2D,
                     image: vk::Image,
                     image_mem: vk::DeviceMemory,
                     view: vk::ImageView)
                     -> Option<Mesh>
    {
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
                unsafe {
                    // bind the texture for our window
                    rend.update_sampler_descriptor_set(
                        descriptors[i],
                        1, //n binding
                        0, // element
                        ctx.image_sampler,
                        view,
                    );
                }
            }

            return Some(Mesh {
                image: image,
                image_view: view,
                image_mem: image_mem,
                image_resolution: *res,
                pool_handle: handle,
                sampler_descriptors: descriptors,
                m_priv: private,
            });
        }
        return None;
    }

    // Create a mesh and its needed data
    //
    // All resources will be allocated by
    // rend
    pub fn update_contents(&mut self,
                           rend: &mut Renderer,
                           data: WindowContents,
                           release: ReleaseInfo)
    {
        match data {
            WindowContents::mem_image(m) =>
                self.update_from_mem_image(rend, m),
            WindowContents::dmabuf(d) =>
                self.update_from_dmabuf(rend, d, release),
        };
    }

    fn update_from_mem_image(&mut self,
                             rend: &mut Renderer,
                             img: &MemImage)
    {
        if let MeshPrivate::mem_image(mp) = &self.m_priv {
            unsafe {
                // copy the data into the staging buffer
                rend.update_memory(mp.transfer_mem,
                                   img.as_slice());
                // copy the staging buffer into the image
                rend.update_image_contents_from_buf(
                    mp.transfer_buf,
                    self.image,
                    self.image_resolution.width,
                    self.image_resolution.height,
                );
            }
        }
    }

    fn update_from_dmabuf(&mut self,
                          rend: &mut Renderer,
                          dmabuf: &Dmabuf,
                          release: ReleaseInfo)
    {
        println!("Updating mesh with dmabuf {:?}", dmabuf);
        if let MeshPrivate::dmabuf(dp) = &mut self.m_priv {
            unsafe {
                // We need to update and rebind the memory
                // for image
                //
                // see from_dmabuf for a complete example
                let mut alloc_info = vk::MemoryAllocateInfo::builder()
                    .allocation_size(dp.dp_mem_reqs.size)
                    .memory_type_index(dp.dp_memtype_index);

                alloc_info.p_next = &vk::ImportMemoryFdInfoKHR::builder()
                    .handle_type(vk::ExternalMemoryHandleTypeFlags
                                 ::EXTERNAL_MEMORY_HANDLE_TYPE_DMA_BUF)
                    // Need to dup the fd, since I think the implementation
                    // will internally free whatever we give it
                    .fd(dup(dmabuf.db_fd).unwrap())
                    as *const _ as *const std::ffi::c_void;

                // perform the import
                let image_memory = rend.dev.allocate_memory(&alloc_info,
                                                            None)
                    .unwrap();

                // Release the old frame's resources
                //
                // Free the old memory and replace it with the new one
                rend.dev.free_memory(self.image_mem, None);
                self.image_mem = image_memory;

                // update the image header with the new import
                rend.dev.bind_image_memory(self.image, self.image_mem, 0)
                    .expect("Unable to rebind device memory to image");

                // the old release info will be implicitly dropped
                // after it has been drawn and presented
                let mut old_release = release;
                // swap our new release info into dp
                mem::swap(&mut dp.dp_release_info, &mut old_release);
                rend.register_for_release(old_release);
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
                // dma has nothing dynamic to free
                MeshPrivate::dmabuf(_) => {},
                MeshPrivate::mem_image(m) => {
                    rend.dev.destroy_buffer(m.transfer_buf, None);
                    rend.dev.free_memory(m.transfer_mem, None);
                },
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
