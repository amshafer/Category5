// A vulkan rendering backend
//
// This layer is very low, and as a result is mostly unsafe. Nothing
// unsafe/vulkan/ash/etc should be exposed to upper layers
//
// Austin Shafer - 2020
#![allow(non_camel_case_types)]
use std::marker::Copy;
use std::sync::Arc;

use ash::vk;

use crate::display::Display;
use crate::instance::Instance;
use crate::{Device, Droppable};

extern crate utils as cat5_utils;
use crate::{CreateInfo, Result};
use cat5_utils::{log, region::Rect};

use lluvia as ll;

pub struct VkBarriers {
    /// Dmabuf import usage barrier list. Will be regenerated
    /// during every draw
    pub r_acquire_barriers: Vec<vk::ImageMemoryBarrier>,
    /// Dmabuf import release barriers. These let drm know vulkan
    /// is done using them.
    pub r_release_barriers: Vec<vk::ImageMemoryBarrier>,
}

// Manually define these for this struct, this is safe since it
// only references vulkan objects.
unsafe impl Send for VkBarriers {}
unsafe impl Sync for VkBarriers {}

/// Common bits of a vulkan renderer
///
/// The fields here are sure to change, as they are pretty
/// application specific.
///
/// The types in ash::vk:: are the 'normal' vulkan types
/// types in ash:: are normally 'loaders'. They take care of loading
/// function pointers and things. Think of them like a wrapper for
/// the raw vk:: type. In some cases you need both, surface
/// is a good example of this.
///
/// Application specific fields should be at the bottom of the
/// struct, with the commonly required fields at the top.
pub struct Renderer {
    /// The instance this rendering context was created from
    pub(crate) _inst: Arc<Instance>,
    /// The GPU this Renderer is resident on
    pub(crate) dev: Arc<Device>,

    /// One sampler for all swapchain images
    pub(crate) image_sampler: vk::Sampler,

    /// The pending release list
    /// This is the set of wayland resources used last frame
    /// for rendering that should now be released
    /// See WindowManger's worker_thread for more
    pub(crate) r_release: Vec<Box<dyn Droppable + Send + Sync>>,

    /// We keep a list of image views from the surface list's images
    /// to be passed as our unsized image array in our shader. This needs
    /// to be regenerated any time a change to the surfacelist is made
    pub(crate) r_images_desc_pool: vk::DescriptorPool,
    pub(crate) r_images_desc_layout: vk::DescriptorSetLayout,
    pub(crate) r_images_desc: vk::DescriptorSet,
    r_images_desc_size: usize,

    /// Temporary image to bind to the image list when
    /// no images are attached.
    tmp_image: vk::Image,
    tmp_image_view: vk::ImageView,
    tmp_image_mem: vk::DeviceMemory,

    // We keep this around to ensure the image array isn't empty
    _r_null_image: ll::Entity,
    pub r_image_ecs: ll::Instance,
    pub r_image_infos: ll::NonSparseComponent<vk::DescriptorImageInfo>,
}

/// Recording parameters
///
/// Layers above this one will need to call recording
/// operations. They need a private structure to pass
/// to Renderer to begin/end recording operations
/// This is that structure.
pub struct RecordParams {
    /// our cached pushbuffer constants
    pub push: PushConstants,
}

impl RecordParams {
    pub fn new() -> Self {
        Self {
            push: PushConstants {
                width: 0,
                height: 0,
                image_id: -1,
                use_color: -1,
                color: (0.0, 0.0, 0.0, 0.0),
                dims: Rect::new(0, 0, 0, 0),
            },
        }
    }
}

/// Shader push constants
///
/// These will be updated when we record the per-viewport draw commands
/// and will contain the scrolling model transformation of all content
/// within a viewport.
///
/// This is also where we pass in the Surface's data.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PushConstants {
    pub width: u32,
    pub height: u32,
    /// The id of the image. This is the offset into the unbounded sampler array.
    /// id that's the offset into the unbound sampler array
    pub image_id: i32,
    /// if we should use color instead of texturing
    pub use_color: i32,
    /// Opaque color
    pub color: (f32, f32, f32, f32),
    /// The complete dimensions of the window.
    pub dims: Rect<i32>,
}

// Most of the functions below will be unsafe. Only the safe functions
// should be used by the applications. The unsafe functions are mostly for
// internal use.
impl Renderer {
    /// Returns true if there are any resources in
    /// the current release list.
    pub fn release_is_empty(&mut self) -> bool {
        return self.r_release.is_empty();
    }

    /// Drop all of the resources, this is used to
    /// release wl_buffers after they have been drawn.
    /// We should not deal with wayland structs
    /// directly, just with releaseinfo
    pub fn release_pending_resources(&mut self) {
        log::profiling!("-- releasing pending resources --");

        // This is the previous frames's pending release list
        // We will clear it, therefore dropping all the relinfos
        self.r_release.clear();
    }

    /// Add a ReleaseInfo to the list of resources to be
    /// freed this frame
    ///
    /// Takes care of choosing what list to add info to
    pub fn register_for_release(&mut self, release: Box<dyn Droppable + Send + Sync>) {
        self.r_release.push(release);
    }

    unsafe fn allocate_bindless_resources(
        dev: &Device,
        max_image_count: u32,
    ) -> (vk::DescriptorPool, vk::DescriptorSetLayout) {
        // create the bindless desc set resources
        let size = [vk::DescriptorPoolSize::builder()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            // Okay it looks like this must match the layout
            // TODO: should this be changed?
            .descriptor_count(max_image_count)
            .build()];
        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);
        let bindless_pool = dev.dev.create_descriptor_pool(&info, None).unwrap();

        let bindings = [
            // the variable image list
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(
                    vk::ShaderStageFlags::COMPUTE
                        | vk::ShaderStageFlags::VERTEX
                        | vk::ShaderStageFlags::FRAGMENT,
                )
                // This is the upper bound on the amount of descriptors that
                // can be attached. The amount actually attached will be
                // determined by the amount allocated using this layout.
                .descriptor_count(max_image_count)
                .build(),
        ];
        let mut info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);

        // We need to attach some binding flags stating that we intend
        // to use the storage image as an unsized array
        let usage_info = vk::DescriptorSetLayoutBindingFlagsCreateInfoEXT::builder()
            .binding_flags(&[
                Self::get_bindless_desc_flags(), // The unbounded array of images
            ])
            .build();
        info.p_next = &usage_info as *const _ as *mut std::ffi::c_void;

        let bindless_layout = dev.dev.create_descriptor_set_layout(&info, None).unwrap();

        (bindless_pool, bindless_layout)
    }

    /// Create a new Vulkan Renderer
    ///
    /// This renderer is very application specific. It is not meant to be
    /// a generic safe wrapper for vulkan. This method constructs a new context,
    /// creating a vulkan instance, finding a physical gpu, setting up a logical
    /// device, and creating a swapchain.
    ///
    /// All methods called after this only need to take a mutable reference to
    /// self, avoiding any nasty argument lists like the functions above.
    /// The goal is to have this make dealing with the api less wordy.
    pub fn new(
        instance: Arc<Instance>,
        dev: Arc<Device>,
        info: &CreateInfo,
        mut img_ecs: ll::Instance,
    ) -> Result<(Renderer, Display)> {
        unsafe {
            // Our display is in charge of choosing a medium to draw on,
            // and will create a surface on that medium
            let display = Display::new(info, dev.clone())?;

            let sampler = dev.create_sampler();

            let (bindless_pool, bindless_layout) =
                // Subtract three resources from the theoretical max that the driver reported.
                // This is to account for our null image and other resources we create in
                // addition to our bindless count.
                Self::allocate_bindless_resources(&dev, dev.dev_features.max_sampler_count - 3);
            let bindless_desc =
                Self::allocate_bindless_desc(&dev, bindless_pool, &[bindless_layout], 1);

            let (tmp, tmp_view, tmp_mem) = dev.create_image(
                &vk::Extent2D {
                    width: 2,
                    height: 2,
                },
                display.d_state.d_surface_format.format,
                vk::ImageUsageFlags::SAMPLED,
                vk::ImageAspectFlags::COLOR,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::ImageTiling::LINEAR,
            );

            // Create the image vk info component
            // We have deleted this image, but it's invalid to pass a
            // NULL VkImageView as a descriptor. Instead we will populate
            // it with our "null"/"tmp" image, which is just a black square
            let null_sampler = sampler;
            let null_view = tmp_view;
            let img_info_comp = img_ecs.add_non_sparse_component(move || {
                vk::DescriptorImageInfo::builder()
                    .sampler(null_sampler)
                    .image_view(null_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .build()
            });

            // Add our null image
            let null_image = img_ecs.add_entity();
            img_info_comp.set(
                &null_image,
                vk::DescriptorImageInfo::builder()
                    .sampler(sampler)
                    .image_view(tmp_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .build(),
            );

            // you are now the proud owner of a half complete
            // rendering context
            // p.s. you still need a Pipeline
            let rend = Renderer {
                _inst: instance,
                dev: dev,
                image_sampler: sampler,
                r_release: Vec::new(),
                r_images_desc_pool: bindless_pool,
                r_images_desc_layout: bindless_layout,
                r_images_desc: bindless_desc,
                r_images_desc_size: 0,
                tmp_image: tmp,
                tmp_image_view: tmp_view,
                tmp_image_mem: tmp_mem,
                _r_null_image: null_image,
                r_image_ecs: img_ecs,
                r_image_infos: img_info_comp,
            };

            return Ok((rend, display));
        }
    }

    /// Wait for the submit_fence
    ///
    /// This waits for the last frame render operation to finish submitting.
    pub fn wait_for_prev_submit(&self) {
        self.dev.wait_for_latest_timeline();
    }

    /// Start recording a cbuf for one frame
    pub fn begin_recording_one_frame(&mut self) -> Result<RecordParams> {
        // At least wait for any image copies to complete
        self.dev.wait_for_copy();

        Ok(RecordParams::new())
    }

    /// End a total frame recording
    pub fn end_recording_one_frame(&mut self) {}

    /// Descriptor flags for the unbounded array of images
    /// we need to say that it is a variably sized array, and that it is partially
    /// bound (aka we aren't populating the full MAX_IMAGE_LIMIT)
    pub fn get_bindless_desc_flags() -> vk::DescriptorBindingFlags {
        vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT
            | vk::DescriptorBindingFlags::PARTIALLY_BOUND
    }

    fn allocate_bindless_desc(
        dev: &Device,
        pool: vk::DescriptorPool,
        layouts: &[vk::DescriptorSetLayout],
        desc_count: u32,
    ) -> vk::DescriptorSet {
        // if thundr has allocated a different number of images than we were expecting,
        // we need to realloc the variable descriptor memory
        let mut info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(layouts)
            .build();
        let variable_info = vk::DescriptorSetVariableDescriptorCountAllocateInfo::builder()
            // This list specifies the number of allocations for the variable
            // descriptor entry in each layout. We only have one layout.
            .descriptor_counts(&[desc_count])
            .build();

        info.p_next = &variable_info as *const _ as *mut std::ffi::c_void;

        unsafe { dev.dev.allocate_descriptor_sets(&info).unwrap()[0] }
    }

    pub fn refresh_window_resources(&mut self) {
        self.wait_for_prev_submit();

        // Construct a list of image views from the submitted surface list
        // this will be our unsized texture array that the composite shader will reference
        // TODO: make this a changed flag
        if self.r_images_desc_size < self.r_image_ecs.capacity() {
            // free the previous descriptor sets
            unsafe {
                self.dev
                    .dev
                    .reset_descriptor_pool(
                        self.r_images_desc_pool,
                        vk::DescriptorPoolResetFlags::empty(),
                    )
                    .unwrap();
            }

            self.r_images_desc_size = self.r_image_ecs.capacity();
            self.r_images_desc = Self::allocate_bindless_desc(
                &self.dev,
                self.r_images_desc_pool,
                &[self.r_images_desc_layout],
                self.r_images_desc_size as u32,
            );
        }

        // Now write the new bindless descriptor
        let write_infos = &[vk::WriteDescriptorSet::builder()
            .dst_set(self.r_images_desc)
            .dst_binding(1)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(self.r_image_infos.get_data_slice().data())
            .build()];
        log::info!(
            "Raw image infos is {:#?}",
            self.r_image_infos.get_data_slice().data()
        );

        unsafe {
            self.dev.dev.update_descriptor_sets(
                write_infos, // descriptor writes
                &[],         // descriptor copies
            );
        }
    }
}

// Clean up after ourselves when the renderer gets destroyed.
//
// This is pretty straightforward, things are destroyed in roughly
// the reverse order that they were created in. Don't forget to add
// new fields of Renderer here if needed.
//
// Could probably use some error checking, but if this gets called we
// are exiting anyway.
impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            log::profiling!("Stoping the renderer");

            // first wait for the device to finish working
            self.dev.dev.device_wait_idle().unwrap();

            self.dev.dev.destroy_image(self.tmp_image, None);
            self.dev.dev.destroy_image_view(self.tmp_image_view, None);
            self.dev.free_memory(self.tmp_image_mem);

            self.dev.dev.destroy_sampler(self.image_sampler, None);
            self.dev
                .dev
                .destroy_descriptor_set_layout(self.r_images_desc_layout, None);

            self.dev
                .dev
                .destroy_descriptor_pool(self.r_images_desc_pool, None);
        }
    }
}
