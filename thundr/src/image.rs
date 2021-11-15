// Images represent a textured quad used to draw 2D
// graphics.
//
// Austin Shafer - 2020
#![allow(dead_code)]
// We allow using dyn Drop here for the drop info fields
// that will be passed around
#![allow(dyn_drop)]
extern crate ash;
extern crate nix;

use super::renderer::Renderer;
use utils::log;
use utils::region::Rect;
use utils::{Dmabuf, MemImage};

use std::cell::RefCell;
use std::ops::Drop;
use std::rc::Rc;
use std::{fmt, iter, mem};

use ash::vk;
use nix::errno::Errno;
use nix::unistd::dup;
use nix::Error;

use crate::{Damage, PipelineType};

/// A image buffer containing contents to be composited.
///
/// An Image will be created from a data source and attached to
/// a Surface. The Surface will contain where on the screen to
/// draw an object, and the Image specifies what pixels to draw.
///
/// Images must be created from the global thundr instance. All
/// images must be destroyed before the instance can be.
pub(crate) struct ImageInternal {
    /// This id is the index of this image in Thundr's image list (th_image_list).
    pub i_id: i32,
    /// Is this image known to the renderer. This is true if the image was destroyed.
    /// If true, it needs to be dropped and recreated.
    pub i_out_of_date: bool,
    /// image containing the contents of the window.
    pub i_image: vk::Image,
    pub i_image_view: vk::ImageView,
    pub i_image_mem: vk::DeviceMemory,
    pub i_image_resolution: vk::Extent2D,
    pub i_pool_handle: usize,
    // TODO: move these into geometric pipeline
    pub i_sampler_descriptors: Vec<vk::DescriptorSet>,
    /// specific to the type of image
    i_priv: ImagePrivate,
    /// Stuff to release when we are no longer using
    /// this gpu buffer (release the wl_buffer)
    i_release_info: Option<Box<dyn Drop>>,
    pub i_damage: Option<Damage>,
    pub i_opaque: Option<Rect<i32>>,
}

impl Image {
    pub(crate) fn assert_valid(&self) {
        assert!(!self.i_internal.borrow().i_out_of_date);
    }

    pub(crate) fn get_view(&self) -> vk::ImageView {
        self.assert_valid();
        self.i_internal.borrow().i_image_view
    }
    pub(crate) fn get_resolution(&self) -> vk::Extent2D {
        self.assert_valid();
        self.i_internal.borrow().i_image_resolution
    }

    /// Sets an opaque region for the image to help the internal compositor
    /// optimize when possible.
    pub fn set_opaque(&mut self, opaque: Option<Rect<i32>>) {
        self.assert_valid();
        self.i_internal.borrow_mut().i_opaque = opaque;
    }

    /// Attach damage to this surface. Damage is specified in surface-coordinates.
    pub fn set_damage(&mut self, x: i32, y: i32, width: i32, height: i32) {
        self.assert_valid();
        // Check if damage is initialized. If it isn't create a new one.
        // If it is, add the damage to the existing list
        let new_rect = Rect::new(x, y, width, height);
        let mut internal = self.i_internal.borrow_mut();
        if let Some(d) = internal.i_damage.as_mut() {
            d.add(&new_rect);
        } else {
            internal.i_damage = Some(Damage::new(vec![new_rect]));
        }
    }

    pub fn reset_damage(&mut self, damage: Damage) {
        self.assert_valid();
        self.i_internal.borrow_mut().i_damage = Some(damage);
        // TODO: clip to image size
    }

    /// set the id. This should only be done by Thundr
    pub(crate) fn set_id(&mut self, id: i32) {
        self.assert_valid();
        self.i_internal.borrow_mut().i_id = id;
    }

    /// Get the id. This is consumed by the pipelines that need to contruct the descriptor
    /// indexing array.
    pub(crate) fn get_id(&self) -> i32 {
        self.i_internal.borrow().i_id
    }

    /// Removes any damage from this image.
    pub fn clear_damage(&mut self) {
        self.i_internal.borrow_mut().i_damage = None;
    }
}

#[derive(Clone)]
pub struct Image {
    pub(crate) i_internal: Rc<RefCell<ImageInternal>>,
}

impl PartialEq for Image {
    /// Two images are equal if their internal data is the same.
    fn eq(&self, other: &Self) -> bool {
        &*self.i_internal.borrow() as *const ImageInternal
            == &*other.i_internal.borrow() as *const ImageInternal
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let image = self.i_internal.borrow();
        f.debug_struct("Image")
            .field("VkImage", &image.i_image)
            .field("Image View", &image.i_image_view)
            .field("Image mem", &image.i_image_mem)
            .field("Resolution", &image.i_image_resolution)
            .field("Pool Handle", &image.i_pool_handle)
            .field("Sampler Descriptors", &image.i_sampler_descriptors)
            .field("Image Private", &image.i_priv)
            .field("Release info", &"<release info omitted>".to_string())
            .finish()
    }
}

/// Private data specific to a image type.
///
/// There are two types of imagees: memimages, and dmabufs
/// MemImages represent shared memory that is copied
/// and used as the image's texture
///
/// Dmabufs are GPU buffers passed by fd. They will be
/// imported (copyless) and bound to the image's image
#[derive(Debug)]
enum ImagePrivate {
    Dmabuf(DmabufPrivate),
    MemImage,
}

/// Private data for gpu buffers
#[derive(Debug)]
struct DmabufPrivate {
    /// we need to cache the params to import memory with
    ///
    /// memory reqs for the image image
    dp_mem_reqs: vk::MemoryRequirements,
    /// the type of memory to use
    dp_memtype_index: u32,
}

impl Renderer {
    /// Create a new image from a shm buffer
    pub fn create_image_from_bits(
        &mut self,
        img: &MemImage,
        _release: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        unsafe {
            let tex_res = vk::Extent2D {
                width: img.width as u32,
                height: img.height as u32,
            };

            // At this point we can drop release. We have already copied from the
            // memimage so we are good to signal wayland

            // This image will back the contents of the on-screen
            // client window.
            // TODO: this should eventually just use the image reported from
            // wayland.
            let (image, view, img_mem) = Renderer::create_image(
                &self.dev,
                &self.mem_props,
                &tex_res,
                vk::Format::B8G8R8A8_UNORM,
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                vk::ImageAspectFlags::COLOR,
                vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_COHERENT
                    | vk::MemoryPropertyFlags::HOST_VISIBLE,
                vk::ImageTiling::LINEAR,
            );

            // Now copy the bits into the image
            self.update_memory(img_mem, 0, img.as_slice());

            // transition us into the appropriate memory layout for shaders
            self.cbuf_begin_recording(self.copy_cbuf, vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            let src_stage = vk::PipelineStageFlags::TOP_OF_PIPE;
            let dst_stage = match self.r_pipe_type {
                PipelineType::GEOMETRIC => vk::PipelineStageFlags::FRAGMENT_SHADER,
                PipelineType::COMPUTE => vk::PipelineStageFlags::COMPUTE_SHADER,
                PipelineType::ALL => {
                    vk::PipelineStageFlags::FRAGMENT_SHADER | vk::PipelineStageFlags::COMPUTE_SHADER
                }
            };
            let layout_barrier = vk::ImageMemoryBarrier::builder()
                .image(image)
                .src_access_mask(vk::AccessFlags::TRANSFER_READ)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .subresource_range(
                    vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1)
                        .level_count(1)
                        .build(),
                )
                .build();
            self.dev.cmd_pipeline_barrier(
                self.copy_cbuf,
                src_stage,
                dst_stage,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[layout_barrier],
            );
            self.cbuf_end_recording(self.copy_cbuf);

            self.cbuf_submit_async(
                self.copy_cbuf,
                self.present_queue,
                &[], // wait_stages
                &[], // wait_semas
                &[], // signal_semas
                self.copy_cbuf_fence,
            );

            return self.create_image_common(
                ImagePrivate::MemImage,
                &tex_res,
                image,
                img_mem,
                view,
                None,
            );
        }
    }

    /// returns the index of the memory type to use
    /// similar to Renderer::find_memory_type_index
    fn find_memtype_for_dmabuf(
        dmabuf_type_bits: u32,
        props: &vk::PhysicalDeviceMemoryProperties,
        reqs: &vk::MemoryRequirements,
    ) -> Option<u32> {
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

    /// Create a new image from a dmabuf
    ///
    /// This is used during the first update of window
    /// contents on an app. It will import the dmabuf
    /// and create an image/view pair representing it.
    pub fn create_image_from_dmabuf(
        &mut self,
        dmabuf: &Dmabuf,
        release: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        log::debug!("Creating image from dmabuf {:?}", dmabuf);
        // A lot of this is duplicated from Renderer::create_image
        unsafe {
            // According to the mesa source, this supports all modifiers
            let target_format = vk::Format::B8G8R8A8_UNORM;
            // get_physical_device_format_properties2
            let mut format_props = vk::FormatProperties2::builder().build();
            let mut drm_fmt_props = vk::DrmFormatModifierPropertiesListEXT::builder().build();
            format_props.p_next = &drm_fmt_props as *const _ as *mut std::ffi::c_void;

            // get the number of drm format mods props
            self.inst.get_physical_device_format_properties2(
                self.pdev,
                target_format,
                &mut format_props,
            );
            let mut mods: Vec<_> = iter::repeat(vk::DrmFormatModifierPropertiesEXT::default())
                .take(drm_fmt_props.drm_format_modifier_count as usize)
                .collect();

            drm_fmt_props.p_drm_format_modifier_properties = mods.as_mut_ptr();
            self.inst.get_physical_device_format_properties2(
                self.pdev,
                target_format,
                &mut format_props,
            );

            for m in mods.iter() {
                log::debug!("dmabuf {} found mod {:#?}", dmabuf.db_fd, m);
            }

            // the parameters to use for image creation
            let mut img_fmt_info = vk::PhysicalDeviceImageFormatInfo2::builder()
                .format(target_format)
                .ty(vk::ImageType::TYPE_2D)
                .usage(vk::ImageUsageFlags::SAMPLED)
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .build();
            let drm_img_props = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::builder()
                .drm_format_modifier(dmabuf.db_mods)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .queue_family_indices(&[self.graphics_family_index])
                .build();
            img_fmt_info.p_next = &drm_img_props as *const _ as *mut std::ffi::c_void;
            // the returned properties
            // the dimensions of the image will be returned here
            let mut img_fmt_props = vk::ImageFormatProperties2::builder().build();
            self.inst
                .get_physical_device_image_format_properties2(
                    self.pdev,
                    &img_fmt_info,
                    &mut img_fmt_props,
                )
                .unwrap();
            log::debug!(
                "dmabuf {} image format properties {:#?} {:#?}",
                dmabuf.db_fd,
                img_fmt_props,
                drm_img_props
            );

            // we create the image now, but will have to bind
            // some memory to it later.
            let mut image_info = vk::ImageCreateInfo::builder()
                .image_type(vk::ImageType::TYPE_2D)
                // TODO: add other formats
                .format(target_format)
                .extent(vk::Extent3D {
                    width: dmabuf.db_width as u32,
                    height: dmabuf.db_height as u32,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                // we are only doing the linear format for now
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .usage(vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let mut ext_mem_info = vk::ExternalMemoryImageCreateInfo::builder()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                .build();
            // ???: Mesa doesn't use this one?
            // let drm_create_info =
            //     vk::ImageDrmFormatModifierExplicitCreateInfoEXT::builder()
            //     .drm_format_modifier(dmabuf.db_mods)
            //     .plane_layouts(&[
            //         vk::SubresourceLayout::builder()
            //             .size(
            //                 (dmabuf.db_stride * dmabuf.db_height as u32) as u64
            //             )
            //             .row_pitch(
            //                 dmabuf.db_stride as u64
            //                     - (dmabuf.db_width * 4) as u64
            //             )
            //             .build()
            //     ])
            //     .build();
            let drm_create_info = vk::ImageDrmFormatModifierListCreateInfoEXT::builder()
                .drm_format_modifiers(&[dmabuf.db_mods])
                .build();
            ext_mem_info.p_next = &drm_create_info as *const _ as *mut std::ffi::c_void;
            image_info.p_next = &ext_mem_info as *const _ as *mut std::ffi::c_void;
            let image = self.dev.create_image(&image_info, None).unwrap();

            // we need to find a memory type that matches the type our
            // new image needs
            let mem_reqs = self.dev.get_image_memory_requirements(image);
            let mem_props = Renderer::get_pdev_mem_properties(&self.inst, self.pdev);
            // supported types we can import as
            let dmabuf_type_bits = self
                .external_mem_fd_loader
                .get_memory_fd_properties_khr(
                    vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                    dmabuf.db_fd,
                )
                .expect("Could not get memory fd properties")
                // bitmask set for each supported memory type
                .memory_type_bits;

            let memtype_index =
                Renderer::find_memtype_for_dmabuf(dmabuf_type_bits, &mem_props, &mem_reqs)
                    .expect("Could not find a memtype for the dmabuf");

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
                .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                // need to dup the fd since it seems the implementation will
                // internally free it
                .fd(dup(dmabuf.db_fd).unwrap()) as *const _
                as *const std::ffi::c_void;

            // perform the import
            let image_memory = self.dev.allocate_memory(&alloc_info, None).unwrap();
            self.dev
                .bind_image_memory(image, image_memory, 0)
                .expect("Unable to bind device memory to image");

            // finally make a view to wrap the image
            let view_info = vk::ImageViewCreateInfo::builder()
                .subresource_range(
                    vk::ImageSubresourceRange::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .level_count(1)
                        .layer_count(1)
                        .build(),
                )
                .image(image)
                .format(image_info.format)
                .view_type(vk::ImageViewType::TYPE_2D);

            let view = self.dev.create_image_view(&view_info, None).unwrap();

            return self.create_image_common(
                ImagePrivate::Dmabuf(DmabufPrivate {
                    dp_mem_reqs: mem_reqs,
                    dp_memtype_index: memtype_index,
                }),
                &vk::Extent2D {
                    width: dmabuf.db_width as u32,
                    height: dmabuf.db_height as u32,
                },
                image,
                image_memory,
                view,
                release,
            );
        }
    }

    /// Create a image
    ///
    /// This logic is the same no matter what type of
    /// resources the image was made from. It allocates
    /// descriptors and constructs the image struct
    fn create_image_common(
        &mut self,
        private: ImagePrivate,
        res: &vk::Extent2D,
        image: vk::Image,
        image_mem: vk::DeviceMemory,
        view: vk::ImageView,
        release: Option<Box<dyn Drop>>,
    ) -> Option<Image> {
        // each image holds a set of descriptors that it will
        // bind before drawing itself. This set holds the
        // image sampler.
        //
        // right now they only hold an image sampler
        let (handle, descriptors) = self.desc_pool.allocate_samplers(&self.dev, self.fb_count);

        for i in 0..self.fb_count {
            unsafe {
                // bind the texture for our window
                self.update_sampler_descriptor_set(
                    descriptors[i],
                    1, //n binding
                    0, // element
                    self.image_sampler,
                    view,
                );
            }
        }

        return Some(Image {
            i_internal: Rc::new(RefCell::new(ImageInternal {
                // The id will be filled in by thundr in lib.rs
                i_id: 0,
                i_out_of_date: false,
                i_image: image,
                i_image_view: view,
                i_image_mem: image_mem,
                i_image_resolution: *res,
                i_pool_handle: handle,
                i_sampler_descriptors: descriptors,
                i_priv: private,
                i_release_info: release,
                i_damage: None,
                i_opaque: None,
            })),
        });
    }

    /// Update image contents from a shm buffer
    ///
    /// If damage is passed, then only those sections of the image will be
    /// updated.
    pub fn update_image_from_bits(
        &mut self,
        thundr_image: &mut Image,
        memimg: &MemImage,
        damage: Option<&Damage>,
        _release: Option<Box<dyn Drop>>,
    ) {
        // we have to take a mut ref to the dereferenced value, so that
        // we get a full mutable borrow of i_internal, which tells rust
        // that we can borrow individual fields later in this function
        let mut image = &mut *thundr_image.i_internal.borrow_mut();
        unsafe {
            log::debug!(
                "update_fr_mem_img: new img is {}x{} and old image_resolution is {:?}",
                memimg.width,
                memimg.height,
                image.i_image_resolution
            );

            // Wait for any operations to complete before touching the buffer
            self.wait_for_prev_submit();
            // If the we are updating with a new size, then we need to recreate the
            // image
            if memimg.width != image.i_image_resolution.width as usize
                || memimg.height != image.i_image_resolution.height as usize
            {
                self.dev.free_memory(image.i_image_mem, None);
                self.dev.destroy_image_view(image.i_image_view, None);
                self.dev.destroy_image(image.i_image, None);
                // we need to re-create & resize the image since we changed
                // the resolution
                let (vkimage, view, img_mem) = Renderer::create_image(
                    &self.dev,
                    &self.mem_props,
                    &vk::Extent2D {
                        width: memimg.width as u32,
                        height: memimg.height as u32,
                    },
                    vk::Format::B8G8R8A8_UNORM,
                    vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                    vk::ImageAspectFlags::COLOR,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL
                        | vk::MemoryPropertyFlags::HOST_COHERENT
                        | vk::MemoryPropertyFlags::HOST_VISIBLE,
                    vk::ImageTiling::LINEAR,
                );
                image.i_image = vkimage;
                image.i_image_view = view;
                image.i_image_mem = img_mem;
                // update our image's resolution
                image.i_image_resolution.width = memimg.width as u32;
                image.i_image_resolution.height = memimg.height as u32;

                // The image samplers are bound to an image view. So once we
                // have recreated the image view, update the sampler descs
                for i in 0..self.fb_count {
                    // bind the texture for our window
                    self.update_sampler_descriptor_set(
                        image.i_sampler_descriptors[i],
                        1, //n binding
                        0, // element
                        self.image_sampler,
                        image.i_image_view,
                    );
                }
            }

            // Perform the copy
            match damage {
                None => self.update_memory(image.i_image_mem, 0, memimg.as_slice()),
                Some(_) => self.update_memory(image.i_image_mem, 0, memimg.as_slice()),
            }
        }

        // We already copied the image, so silently drop the release info
        self.update_common(&mut image, None);
    }

    /// Update image contents from a GPU buffer
    ///
    /// GPU buffers are passed as dmabuf fds, we will perform
    /// an import using vulkan's external memory extensions
    pub fn update_image_from_dmabuf(
        &mut self,
        thundr_image: &mut Image,
        dmabuf: &Dmabuf,
        release: Option<Box<dyn Drop>>,
    ) {
        let mut image = thundr_image.i_internal.borrow_mut();
        log::debug!("Updating image with dmabuf {:?}", dmabuf);
        if let ImagePrivate::Dmabuf(dp) = &mut image.i_priv {
            // Since we are VERY async/threading friendly here, it is
            // possible that the fd may be bad since the program that
            // owns it was killed. If that is the case just return and
            // don't update the texture.
            let fd = match dup(dmabuf.db_fd) {
                Ok(f) => f,
                Err(Error::Sys(Errno::EBADF)) => return,
                Err(e) => {
                    log::debug!("could not dup fd {:?}", e);
                    return;
                }
            };

            unsafe {
                // We need to update and rebind the memory
                // for image
                //
                // see from_dmabuf for a complete example
                let mut alloc_info = vk::MemoryAllocateInfo::builder()
                    .allocation_size(dp.dp_mem_reqs.size)
                    .memory_type_index(dp.dp_memtype_index)
                    .build();

                let import_info = vk::ImportMemoryFdInfoKHR::builder()
                    .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                    // Need to dup the fd, since I think the implementation
                    // will internally free whatever we give it
                    .fd(fd)
                    .build();

                alloc_info.p_next = &import_info as *const _ as *const std::ffi::c_void;

                // perform the import
                let image_memory = self.dev.allocate_memory(&alloc_info, None).unwrap();

                // Release the old frame's resources
                //
                // Free the old memory and replace it with the new one
                self.free_memory(image.i_image_mem);
                image.i_image_mem = image_memory;

                // update the image header with the new import
                self.dev
                    .bind_image_memory(image.i_image, image.i_image_mem, 0)
                    .expect("Unable to rebind device memory to image");
            }
        } else {
            panic!("Updating non-memimg Image with MemImg");
        }
        self.update_common(&mut image, release);
    }

    /// Common path for updating a image with a new buffer
    fn update_common(&mut self, image: &mut ImageInternal, release: Option<Box<dyn Drop>>) {
        // the old release info will be implicitly dropped
        // after it has been drawn and presented
        let mut old_release = release;
        // swap our new release info into dp
        mem::swap(&mut image.i_release_info, &mut old_release);
        if let Some(old) = old_release {
            self.register_for_release(old);
        }
    }

    /// A simple teardown function. The renderer is needed since
    /// it allocated all these objects.
    pub fn destroy_image(&mut self, thundr_image: &Image) {
        thundr_image.assert_valid();
        let mut image = thundr_image.i_internal.borrow_mut();
        unsafe {
            // Mark that this image was destroyed
            image.i_out_of_date = true;
            self.dev.destroy_image(image.i_image, None);
            self.dev.destroy_image_view(image.i_image_view, None);
            self.free_memory(image.i_image_mem);
            match &image.i_priv {
                // dma has nothing dynamic to free
                ImagePrivate::Dmabuf(_) => {}
                ImagePrivate::MemImage => {}
            }
            // free our descriptors
            self.desc_pool.destroy_samplers(
                &self.dev,
                image.i_pool_handle,
                image.i_sampler_descriptors.as_slice(),
            );
        }
    }
}
