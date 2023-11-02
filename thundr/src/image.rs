// Images represent a textured quad used to draw 2D
// graphics.
//
// Austin Shafer - 2020
#![allow(dead_code)]
extern crate ash;
extern crate lluvia as ll;
extern crate nix;

use super::renderer::Renderer;
use utils::log;
use utils::region::Rect;
use utils::Dmabuf;

use std::fmt;
use std::ops::Drop;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex, RwLock};

use ash::vk;
use nix::fcntl::{fcntl, FcntlArg};
use nix::Error;

use crate::{Damage, Droppable};

// For now we only support one format.
// According to the mesa source, this supports all modifiers.
const TARGET_FORMAT: vk::Format = vk::Format::B8G8R8A8_UNORM;

/// These are the fields private to the vulkan system, mainly
/// the VkImage and other resources that we need to drop once they
/// are unreffed in the renderer.
pub struct ImageVk {
    iv_rend: Arc<Mutex<Renderer>>,
    /// image containing the contents of the window.
    pub iv_image: vk::Image,
    pub iv_image_view: vk::ImageView,
    pub iv_image_mem: vk::DeviceMemory,
    pub iv_image_resolution: vk::Extent2D,
    /// Stuff to release when we are no longer using
    /// this gpu buffer (release the wl_buffer)
    iv_release_info: Option<Box<dyn Droppable + Send + Sync>>,
}

impl Drop for ImageVk {
    /// A simple teardown function. The renderer is needed since
    /// it allocated all these objects.
    fn drop(&mut self) {
        let rend = self.iv_rend.lock().unwrap();
        rend.wait_for_prev_submit();
        log::debug!("Deleting image view {:?}", self.iv_image_view);

        unsafe {
            rend.dev.destroy_image(self.iv_image, None);
            rend.dev.destroy_image_view(self.iv_image_view, None);
            rend.free_memory(self.iv_image_mem);
        }
    }
}

/// A image buffer containing contents to be composited.
///
/// An Image will be created from a data source and attached to
/// a Surface. The Surface will contain where on the screen to
/// draw an object, and the Image specifies what pixels to draw.
///
/// Images must be created from the global thundr instance. All
/// images must be destroyed before the instance can be.
pub(crate) struct ImageInternal {
    i_rend: Arc<Mutex<Renderer>>,
    /// This id is the index of this image in Thundr's image list (th_image_list).
    pub i_id: ll::Entity,
    i_general_layout: bool,
    /// specific to the type of image
    i_priv: ImagePrivate,
    pub i_opaque: Option<Rect<i32>>,
}

impl ImageInternal {
    /// Gets a copy of the image's damage, if it has one.
    ///
    /// Note: potentially expensive copy
    pub fn get_damage(&self) -> Option<Damage> {
        if let Some(d) = self.i_rend.lock().unwrap().r_image_damage.get(&self.i_id) {
            Some((*d).clone())
        } else {
            None
        }
    }
}

impl Image {
    pub(crate) fn get_view(&self) -> vk::ImageView {
        let internal = self.i_internal.write().unwrap();
        let rend = internal.i_rend.lock().unwrap();

        let image_vk = rend.r_image_vk.get(&internal.i_id).unwrap();
        return image_vk.iv_image_view;
    }

    pub fn get_size(&self) -> (u32, u32) {
        let internal = self.i_internal.write().unwrap();
        let rend = internal.i_rend.lock().unwrap();

        let image_vk = rend.r_image_vk.get(&internal.i_id).unwrap();
        (
            image_vk.iv_image_resolution.width,
            image_vk.iv_image_resolution.height,
        )
    }

    /// Sets an opaque region for the image to help the internal compositor
    /// optimize when possible.
    pub fn set_opaque(&mut self, opaque: Option<Rect<i32>>) {
        self.i_internal.write().unwrap().i_opaque = opaque;
    }

    /// Attach damage to this surface. Damage is specified in surface-coordinates.
    pub fn set_damage(&mut self, x: i32, y: i32, width: i32, height: i32) {
        let internal = self.i_internal.write().unwrap();
        let mut rend = internal.i_rend.lock().unwrap();
        // Check if damage is initialized. If it isn't create a new one.
        // If it is, add the damage to the existing list
        let new_rect = Rect::new(x, y, width, height);
        if let Some(mut d) = rend.r_image_damage.get_mut(&internal.i_id) {
            d.add(&new_rect);
            return;
        }

        rend.r_image_damage
            .set(&internal.i_id, Damage::new(vec![new_rect]));
    }

    pub fn reset_damage(&mut self, damage: Damage) {
        let internal = self.i_internal.write().unwrap();
        let mut rend = internal.i_rend.lock().unwrap();

        rend.r_image_damage.set(&internal.i_id, damage);
        // TODO: clip to image size
    }

    /// Get the id. This is consumed by the pipelines that need to contruct the descriptor
    /// indexing array.
    pub(crate) fn get_id(&self) -> ll::Entity {
        self.i_internal.read().unwrap().i_id.clone()
    }

    /// Removes any damage from this image.
    pub fn clear_damage(&self) {
        let internal = self.i_internal.write().unwrap();
        let mut rend = internal.i_rend.lock().unwrap();

        rend.r_image_damage.take(&internal.i_id);
    }
}

#[derive(Clone)]
pub struct Image {
    pub(crate) i_internal: Arc<RwLock<ImageInternal>>,
}

impl PartialEq for Image {
    /// Two images are equal if their internal data is the same.
    fn eq(&self, other: &Self) -> bool {
        &*self.i_internal.read().unwrap() as *const ImageInternal
            == &*other.i_internal.read().unwrap() as *const ImageInternal
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let image = self.i_internal.read().unwrap();
        f.debug_struct("Image")
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
    InvalidImage,
    Dmabuf(DmabufPrivate),
    MemImage,
}

impl Default for ImagePrivate {
    fn default() -> Self {
        Self::InvalidImage
    }
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
    /// Helper that unifies the call for allocating a bgra image
    unsafe fn alloc_bgra8_image(
        &self,
        resolution: &vk::Extent2D,
    ) -> (vk::Image, vk::ImageView, vk::DeviceMemory) {
        Renderer::create_image(
            &self.dev,
            &self.mem_props,
            resolution,
            TARGET_FORMAT,
            vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
            vk::ImageAspectFlags::COLOR,
            vk::MemoryPropertyFlags::DEVICE_LOCAL
                | vk::MemoryPropertyFlags::HOST_COHERENT
                | vk::MemoryPropertyFlags::HOST_VISIBLE,
            vk::ImageTiling::LINEAR,
        )
    }

    /// Create a new image from a shm buffer
    pub fn create_image_from_bits(
        &mut self,
        rend_mtx: Arc<Mutex<Renderer>>,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32,
        _release: Option<Box<dyn Droppable + Send + Sync>>,
    ) -> Option<Image> {
        unsafe {
            let tex_res = vk::Extent2D {
                width: width,
                height: height,
            };

            log::debug!("create_image_from_bits: Image {}x{}", width, height,);

            //log::error!(
            //    "create_image_from_bits: Image {}x{} checksum {}",
            //    img.width,
            //    img.height,
            //    img.checksum()
            //);

            // At this point we can drop release. We have already copied from the
            // memimage so we are good to signal wayland

            // This image will back the contents of the on-screen
            // client window.
            let (image, view, img_mem) = self.alloc_bgra8_image(&tex_res);

            self.update_image_from_data(image, data, width, height, stride);

            return self.create_image_common(
                rend_mtx,
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
        for (i, ref _mem_type) in props.memory_types.iter().enumerate() {
            // Bit i of memoryBitTypes will be set if the resource supports
            // the ith memory type in props.
            //
            // Don't check for DEVICE_LOCAL here, since the dmabuf may be
            // a sysmem buffer.
            //
            // if this index is supported by dmabuf
            if (dmabuf_type_bits >> i) & 1 == 1
                // and by the image
                && (reqs.memory_type_bits >> i) & 1 == 1
            {
                return Some(i as u32);
            }
        }

        return None;
    }

    unsafe fn acquire_dmabuf_image_from_external_queue(&mut self, image: vk::Image) {
        self.wait_for_prev_submit();
        self.wait_for_copy();

        self.dev.reset_fences(&[self.submit_fence]).unwrap();

        // now perform the copy
        self.cbuf_begin_recording(self.copy_cbuf, vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        let acquire_barrier = vk::ImageMemoryBarrier::builder()
            .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
            .dst_queue_family_index(self.graphics_family_index)
            .image(image)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
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
            vk::PipelineStageFlags::TOP_OF_PIPE, // src
            vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::VERTEX_SHADER
                | vk::PipelineStageFlags::COMPUTE_SHADER, // dst
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[acquire_barrier],
        );

        self.cbuf_end_recording(self.copy_cbuf);
        self.cbuf_submit_async(
            self.copy_cbuf,
            self.present_queue,
            &[], // wait_stages
            &[], // wait_semas
            &[], // signal_semas
            self.submit_fence,
        );
        self.wait_for_prev_submit();
    }

    unsafe fn create_dmabuf_image(
        &mut self,
        dmabuf: &Dmabuf,
        dmabuf_priv: &mut DmabufPrivate,
    ) -> Result<(vk::Image, vk::ImageView, vk::DeviceMemory), Error> {
        // Allocate an external image
        // -------------------------------------------------------
        // we create the image now, but will have to bind
        // some memory to it later.
        let mut image_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            // TODO: add other formats
            .format(TARGET_FORMAT)
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
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .build();
        let mut ext_mem_info = vk::ExternalMemoryImageCreateInfo::builder()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .build();

        let drm_create_info = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::builder()
            .drm_format_modifier(dmabuf.db_mods)
            .plane_layouts(&[vk::SubresourceLayout::builder()
                .offset(dmabuf.db_offset as u64)
                .row_pitch(dmabuf.db_stride as u64)
                .size(0)
                .build()])
            .build();
        ext_mem_info.p_next = &drm_create_info as *const _ as *mut std::ffi::c_void;
        image_info.p_next = &ext_mem_info as *const _ as *mut std::ffi::c_void;
        let image = self.dev.create_image(&image_info, None).unwrap();

        // Update the private tracker with memory info
        // -------------------------------------------------------
        // supported types we can import as
        let dmabuf_type_bits = self
            .external_mem_fd_loader
            .get_memory_fd_properties(
                vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                dmabuf.db_fd.as_raw_fd(),
            )
            .expect("Could not get memory fd properties")
            // bitmask set for each supported memory type
            .memory_type_bits;
        // we need to find a memory type that matches the type our
        // new image needs
        dmabuf_priv.dp_mem_reqs = self.dev.get_image_memory_requirements(image);
        let mem_props = Renderer::get_pdev_mem_properties(&self.inst, self.pdev);

        dmabuf_priv.dp_memtype_index = Renderer::find_memtype_for_dmabuf(
            dmabuf_type_bits,
            &mem_props,
            &dmabuf_priv.dp_mem_reqs,
        )
        .expect("Could not find a memtype for the dmabuf");

        //
        // -------------------------------------------------------
        // TODO: use some of these to verify dmabuf imports:
        //
        // VkPhysicalDeviceExternalBufferInfo
        // VkPhysicalDeviceExternalImageInfo

        // We need to import from the dmabuf fd, so we will
        // add a VkImportMemoryFdInfoKHR struct to the next ptr
        // here to tell vulkan that we should import mem
        // instead of allocating it.
        let mut alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(dmabuf.db_stride as u64 * dmabuf.db_height as u64)
            .memory_type_index(dmabuf_priv.dp_memtype_index);

        // Since we are VERY async/threading friendly here, it is
        // possible that the fd may be bad since the program that
        // owns it was killed. If that is the case just return and
        // don't update the texture.
        let fd = match fcntl(dmabuf.db_fd.as_raw_fd(), FcntlArg::F_DUPFD_CLOEXEC(0)) {
            Ok(f) => f,
            Err(e) => {
                log::debug!("could not dup fd {:?}", e);
                return Err(e);
            }
        };
        let mut import_fd_info = vk::ImportMemoryFdInfoKHR::builder()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            // need to dup the fd since it seems the implementation will
            // internally free it
            .fd(fd);

        let dedicated_alloc_info = vk::MemoryDedicatedAllocateInfo::builder()
            .image(image)
            .build();

        import_fd_info.p_next = &dedicated_alloc_info as *const _ as *const std::ffi::c_void;
        alloc_info.p_next = &import_fd_info as *const _ as *const std::ffi::c_void;

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

        self.acquire_dmabuf_image_from_external_queue(image);

        log::debug!(
            "Created Vulkan image {:?} from dmabuf {}",
            image,
            dmabuf.db_fd.as_raw_fd(),
        );
        Ok((image, view, image_memory))
    }

    /// Create a new image from a dmabuf
    ///
    /// This is used during the first update of window
    /// contents on an app. It will import the dmabuf
    /// and create an image/view pair representing it.
    pub fn create_image_from_dmabuf(
        &mut self,
        rend_mtx: Arc<Mutex<Renderer>>,
        dmabuf: &Dmabuf,
        release: Option<Box<dyn Droppable + Send + Sync>>,
    ) -> Option<Image> {
        self.wait_for_prev_submit();
        self.wait_for_copy();

        log::debug!("Updating new image with dmabuf {:?}", dmabuf);
        // A lot of this is duplicated from Renderer::create_image
        unsafe {
            // Check validity of dmabuf format and print info
            // -------------------------------------------------------

            #[cfg(debug_assertions)]
            {
                use std::iter;

                // get_physical_device_format_properties2
                let mut format_props = vk::FormatProperties2::builder().build();
                let mut drm_fmt_props = vk::DrmFormatModifierPropertiesListEXT::builder().build();
                format_props.p_next = &drm_fmt_props as *const _ as *mut std::ffi::c_void;

                // get the number of drm format mods props
                self.inst.get_physical_device_format_properties2(
                    self.pdev,
                    TARGET_FORMAT,
                    &mut format_props,
                );
                let mut mods: Vec<_> = iter::repeat(vk::DrmFormatModifierPropertiesEXT::default())
                    .take(drm_fmt_props.drm_format_modifier_count as usize)
                    .collect();

                drm_fmt_props.p_drm_format_modifier_properties = mods.as_mut_ptr();
                self.inst.get_physical_device_format_properties2(
                    self.pdev,
                    TARGET_FORMAT,
                    &mut format_props,
                );

                for m in mods.iter() {
                    log::debug!("dmabuf {} found mod {:#?}", dmabuf.db_fd.as_raw_fd(), m);
                }
            }

            // the parameters to use for image creation
            let mut img_fmt_info = vk::PhysicalDeviceImageFormatInfo2::builder()
                .format(TARGET_FORMAT)
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
            // -------------------------------------------------------
            log::debug!(
                "dmabuf {} image format properties {:#?} {:#?}",
                dmabuf.db_fd.as_raw_fd(),
                img_fmt_props,
                drm_img_props
            );

            // Make Dmabuf private struct
            // -------------------------------------------------------
            // This will be updated by create_dmabuf_image
            let mut dmabuf_priv = DmabufPrivate {
                dp_mem_reqs: vk::MemoryRequirements::builder().build(),
                dp_memtype_index: 0,
            };
            // Import the dmabuf
            // -------------------------------------------------------
            let (image, view, image_memory) =
                match self.create_dmabuf_image(&dmabuf, &mut dmabuf_priv) {
                    Ok((i, v, im)) => (i, v, im),
                    Err(_e) => {
                        log::debug!("Could not update dmabuf image: {:?}", _e);
                        return None;
                    }
                };

            return self.create_image_common(
                rend_mtx,
                ImagePrivate::Dmabuf(dmabuf_priv),
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

    /// Update the `VkDescriptorImageInfo` entry in the image ECS for the renderer
    ///
    /// This updates the descriptor info we pass to Vulkan describing our images.
    fn update_image_vk_info(&mut self, internal: &ImageInternal) {
        let view = self
            .r_image_vk
            .get(&internal.i_id)
            .as_ref()
            .unwrap()
            .iv_image_view;

        log::info!(
            "Image list index {}: writing view {:?}",
            internal.i_id.get_raw_id(),
            view
        );
        self.r_image_infos.set(
            &internal.i_id,
            vk::DescriptorImageInfo::builder()
                .sampler(self.image_sampler)
                // The image view could have been recreated and this would be stale
                .image_view(view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .build(),
        );
    }

    /// Create a image
    ///
    /// This logic is the same no matter what type of
    /// resources the image was made from. It allocates
    /// descriptors and constructs the image struct
    fn create_image_common(
        &mut self,
        rend_mtx: Arc<Mutex<Renderer>>,
        private: ImagePrivate,
        res: &vk::Extent2D,
        image: vk::Image,
        image_mem: vk::DeviceMemory,
        view: vk::ImageView,
        release: Option<Box<dyn Droppable + Send + Sync>>,
    ) -> Option<Image> {
        let image_vk = ImageVk {
            iv_rend: rend_mtx.clone(),
            iv_image: image,
            iv_image_view: view,
            iv_image_mem: image_mem,
            iv_image_resolution: *res,
            iv_release_info: release,
        };

        let internal = ImageInternal {
            i_rend: rend_mtx,
            i_id: self.r_image_ecs.add_entity(),
            i_general_layout: false,
            i_priv: private,
            i_opaque: None,
        };

        // Add our vulkan resources to the ECS
        self.r_image_vk.set(&internal.i_id, image_vk);

        self.update_image_vk_info(&internal);

        return Some(Image {
            i_internal: Arc::new(RwLock::new(internal)),
        });
    }

    /// Add a Cmd full of image barriers for all images that need it.
    /// This should be called before cbuf_end_recording in the renderer's
    /// big cbuf.
    pub unsafe fn add_image_barriers_for_dmabuf_images(
        &mut self,
        cbuf: vk::CommandBuffer,
        images: &[Image],
    ) {
        self.r_barriers.r_acquire_barriers.clear();
        self.r_barriers.r_release_barriers.clear();

        for img_rc in images.iter() {
            let mut img = img_rc.i_internal.write().unwrap();
            let image_vk = self.r_image_vk.get_mut(&img.i_id).unwrap();

            let src_layout = match img.i_general_layout {
                true => vk::ImageLayout::GENERAL,
                false => vk::ImageLayout::UNDEFINED,
            };
            img.i_general_layout = true;

            self.r_barriers.r_acquire_barriers.push(
                vk::ImageMemoryBarrier::builder()
                    .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                    .dst_queue_family_index(self.graphics_family_index)
                    .image(image_vk.iv_image)
                    .old_layout(src_layout)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .subresource_range(
                        vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1)
                            .level_count(1)
                            .build(),
                    )
                    .build(),
            );
            self.dev.cmd_pipeline_barrier(
                cbuf,
                vk::PipelineStageFlags::TOP_OF_PIPE, // src
                vk::PipelineStageFlags::FRAGMENT_SHADER
                    | vk::PipelineStageFlags::VERTEX_SHADER
                    | vk::PipelineStageFlags::COMPUTE_SHADER, // dst
                vk::DependencyFlags::empty(),
                &[],
                &[],
                self.r_barriers.r_acquire_barriers.as_slice(),
            );

            self.r_barriers.r_release_barriers.push(
                vk::ImageMemoryBarrier::builder()
                    .src_queue_family_index(self.graphics_family_index)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                    .image(image_vk.iv_image)
                    .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .new_layout(vk::ImageLayout::GENERAL)
                    .src_access_mask(vk::AccessFlags::SHADER_READ)
                    .dst_access_mask(vk::AccessFlags::empty())
                    .subresource_range(
                        vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .layer_count(1)
                            .level_count(1)
                            .build(),
                    )
                    .build(),
            );
            self.dev.cmd_pipeline_barrier(
                cbuf,
                vk::PipelineStageFlags::ALL_GRAPHICS | vk::PipelineStageFlags::COMPUTE_SHADER, // src
                vk::PipelineStageFlags::BOTTOM_OF_PIPE, // dst
                vk::DependencyFlags::empty(),
                &[],
                &[],
                self.r_barriers.r_release_barriers.as_slice(),
            );
        }
    }
}
