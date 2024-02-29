// A vulkan rendering backend
//
// This layer is very low, and as a result is mostly unsafe. Nothing
// unsafe/vulkan/ash/etc should be exposed to upper layers
//
// Austin Shafer - 2020
#![allow(dead_code, non_camel_case_types)]
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::marker::Copy;
use std::os::raw::c_void;
use std::sync::Arc;

use ash::extensions::khr;
use ash::vk;

use crate::descpool::DescPool;
use crate::display::Display;
use crate::image::ImageVk;
use crate::instance::Instance;
use crate::list::SurfaceList;
use crate::pipelines::PipelineType;
use crate::platform::VKDeviceFeatures;
use crate::{Device, Droppable, Surface, Viewport};

extern crate utils as cat5_utils;
use crate::{CreateInfo, Damage};
use crate::{Result, ThundrError};
use cat5_utils::{log, region::Rect};

use lluvia as ll;

/// This is the offset from the base of the winlist buffer to the
/// window array in the actual ssbo. This needs to match the `offset`
/// field in the `layout` qualifier in the shaders
pub const WINDOW_LIST_GLSL_OFFSET: isize = 16;

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
    pub(crate) inst: Arc<Instance>,
    /// The GPU this Renderer is resident on
    pub(crate) dev: Arc<Device>,

    /// loads swapchain extension
    pub(crate) swapchain_loader: khr::Swapchain,
    /// the actual swapchain
    pub(crate) swapchain: vk::SwapchainKHR,
    /// index into swapchain images that we are currently using
    pub(crate) current_image: u32,

    /// a set of images belonging to swapchain
    pub(crate) images: Vec<vk::Image>,
    /// One sampler for all swapchain images
    pub(crate) image_sampler: vk::Sampler,
    /// number of framebuffers (2 is double buffering)
    pub(crate) fb_count: usize,
    /// views describing how to access the images
    pub(crate) views: Vec<vk::ImageView>,
    /// The age of the swapchain image. This is equal to the number
    /// of frames it has been since this image was drawn/presented.
    /// This is indexed by `current_image`.
    pub(crate) swap_ages: Vec<usize>,
    /// The lists of regions to pass to vkPresentRegionsKHR. This
    /// allows us to only present the changed regions. This is calculated
    /// from the damages present in the `SurfaceList`.
    pub(crate) damage_regions: VecDeque<Vec<vk::RectLayerKHR>>,
    /// This is the compiled damage regions from all surfacelists rendered. it
    /// will be added to global damage sources and placed in current_damage
    surfacelist_regions: Vec<vk::RectLayerKHR>,
    /// This is the final compiled set of damages for this frame.
    pub(crate) current_damage: Vec<vk::RectLayerKHR>,

    /// Graphics queue family to use. This comes from the Display
    pub(crate) graphics_queue_family: u32,
    /// processes things to be physically displayed
    pub(crate) r_present_queue: vk::Queue,
    /// pools provide the memory allocated to command buffers
    pub(crate) pool: vk::CommandPool,
    /// the command buffers allocated from pool
    pub(crate) cbufs: Vec<vk::CommandBuffer>,
    /// This signals that the latest contents have been presented.
    /// It is signaled by acquire next image and is consumed by
    /// the cbuf submission
    pub(crate) present_sema: vk::Semaphore,
    /// This is signaled by start_frame, and is consumed by present.
    /// This keeps presentation from occurring until rendering is
    /// complete
    pub(crate) render_sema: vk::Semaphore,
    /// This fence coordinates draw call reuse. It will be signaled
    /// when submitting the draw calls to the queue has finished
    pub(crate) submit_fence: vk::Fence,
    /// needed for VkGetMemoryFdPropertiesKHR
    pub(crate) external_mem_fd_loader: khr::ExternalMemoryFd,
    /// The pending release list
    /// This is the set of wayland resources used last frame
    /// for rendering that should now be released
    /// See WindowManger's worker_thread for more
    pub(crate) r_release: Vec<Box<dyn Droppable + Send + Sync>>,
    /// This is an allocator for the dynamic sets (samplers)
    pub(crate) desc_pool: DescPool,

    /// Has vkQueueSubmit been called.
    pub(crate) draw_call_submitted: bool,

    /// The type of pipeline(s) being in use
    pub(crate) r_pipe_type: PipelineType,

    /// Our ECS
    pub r_ecs: ll::Instance,

    /// We keep a list of image views from the surface list's images
    /// to be passed as our unsized image array in our shader. This needs
    /// to be regenerated any time a change to the surfacelist is made
    pub(crate) r_images_desc_pool: vk::DescriptorPool,
    pub(crate) r_images_desc_layout: vk::DescriptorSetLayout,
    pub(crate) r_images_desc: vk::DescriptorSet,
    r_images_desc_size: usize,

    /// The descriptor layout for the surface list's window order desc
    pub r_order_desc_layout: vk::DescriptorSetLayout,

    /// The list of window dimensions that is passed to the shader
    pub r_windows: ll::Component<Window>,
    pub r_windows_buf: vk::Buffer,
    pub r_windows_mem: vk::DeviceMemory,
    /// The number of Windows that r_winlist_mem was allocate to hold
    pub r_windows_capacity: usize,

    /// Temporary image to bind to the image list when
    /// no images are attached.
    tmp_image: vk::Image,
    tmp_image_view: vk::ImageView,
    tmp_image_mem: vk::DeviceMemory,

    /// Memory barriers.
    pub r_barriers: VkBarriers,

    // We keep this around to ensure the image array isn't empty
    r_null_image: ll::Entity,
    pub r_image_ecs: ll::Instance,
    pub r_image_vk: ll::Component<ImageVk>,
    pub r_image_infos: ll::NonSparseComponent<vk::DescriptorImageInfo>,

    /// Identical to the parent Thundr struct's session
    pub r_surface_pass: ll::Component<usize>,
}

/// This must match the definition of the Window struct in the
/// visibility shader.
///
/// This *MUST* be a power of two, as the layout of the shader ssbo
/// is dependent on offsetting using the size of this.
#[repr(C)]
#[derive(Default, Copy, Clone, Serialize, Deserialize, Debug)]
pub struct Window {
    /// The id of the image. This is the offset into the unbounded sampler array.
    /// id that's the offset into the unbound sampler array
    pub w_id: i32,
    /// if we should use w_color instead of texturing
    pub w_use_color: i32,
    /// the render pass count
    pub w_pass: i32,
    /// Padding to match our shader's struct
    w_padding: i32,
    /// Opaque color
    pub w_color: (f32, f32, f32, f32),
    /// The complete dimensions of the window.
    pub w_dims: Rect<i32>,
    /// Opaque region that tells the shader that we do not need to blend.
    /// This will have a r_pos.0 of -1 if no opaque data was attached.
    pub w_opaque: Rect<i32>,
}

/// Recording parameters
///
/// Layers above this one will need to call recording
/// operations. They need a private structure to pass
/// to Renderer to begin/end recording operations
/// This is that structure.
pub struct RecordParams {
    pub cbuf: vk::CommandBuffer,
    pub image_num: usize,
    /// This calculates the depth we should use when starting to draw
    /// a set of surfaces in a viewport.
    ///
    /// This will start at 1.0, the max depth. Every surface will draw
    /// itself starting_depth - (gl_InstanceIndex * 0.00000001) away from the max
    /// depth. We will update this after every draw to calculate the
    /// new depth to offset from, so we don't collide with previously drawn
    /// surfaces in a different viewport.
    pub starting_depth: f32,
}

/// Shader push constants
///
/// These will be updated when we record the per-viewport draw commands
/// and will contain the scrolling model transformation of all content
/// within a viewport.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PushConstants {
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub width: f32,
    pub height: f32,
    pub starting_depth: f32,
}

// Most of the functions below will be unsafe. Only the safe functions
// should be used by the applications. The unsafe functions are mostly for
// internal use.
impl Renderer {
    /// Tear down all the swapchain-dependent vulkan objects we have created.
    /// This will be used when dropping everything and when we need to handle
    /// OOD events.
    unsafe fn destroy_swapchain(&mut self) {
        // Don't destroy the images here, the destroy swapchain call
        // will take care of them
        for view in self.views.iter() {
            self.dev.dev.destroy_image_view(*view, None);
        }
        self.views.clear();

        self.dev
            .dev
            .free_command_buffers(self.pool, self.cbufs.as_slice());
        self.cbufs.clear();

        self.swapchain_loader
            .destroy_swapchain(self.swapchain, None);
        self.swapchain = vk::SwapchainKHR::null();
    }

    /// Recreate our swapchain.
    ///
    /// This will be done on VK_ERROR_OUT_OF_DATE_KHR, signifying that
    /// the window is being resized and we have to regenerate accordingly.
    /// Keep in mind the Pipeline in Thundr will also have to be recreated
    /// separately.
    pub unsafe fn recreate_swapchain(&mut self, display: &mut Display) {
        // first wait for the device to finish working
        self.dev.dev.device_wait_idle().unwrap();

        // We need to get the updated size of our swapchain. This
        // will be the current size of the surface in use. We should
        // also update Display.d_resolution while we are at it.
        let new_res = display.get_vulkan_drawable_size(self.dev.pdev);
        // TODO: clamp resolution here
        display.d_resolution = new_res;

        let new_swapchain = Renderer::create_swapchain(
            display,
            &self.swapchain_loader,
            &self.dev.dev_features,
            self.r_pipe_type,
            Some(self.swapchain), // oldSwapChain
        );

        // Now that we recreated the swapchain destroy the old one
        self.destroy_swapchain();
        self.swapchain = new_swapchain;

        let (images, views) = Renderer::select_images_and_views(
            display,
            &self.inst.inst,
            &self.swapchain_loader,
            self.swapchain,
            &self.dev,
        );
        self.images = images;
        self.views = views;

        self.cbufs = self
            .dev
            .create_command_buffers(self.pool, self.images.len() as u32);
    }

    /// create a new vkSwapchain
    ///
    /// Swapchains contain images that can be used for WSI presentation
    /// They take a vkSurfaceKHR and provide a way to manage swapping
    /// effects such as double/triple buffering (mailbox mode). The created
    /// swapchain is dependent on the characteristics and format of the surface
    /// it is created for.
    /// The application resolution is set by this method.
    unsafe fn create_swapchain(
        display: &mut Display,
        swapchain_loader: &khr::Swapchain,
        dev_features: &VKDeviceFeatures,
        _pipe_type: PipelineType,
        old_swapchain: Option<vk::SwapchainKHR>,
    ) -> vk::SwapchainKHR {
        // how many images we want the swapchain to contain
        let mut desired_image_count = display.d_surface_caps.min_image_count + 1;
        if display.d_surface_caps.max_image_count > 0
            && desired_image_count > display.d_surface_caps.max_image_count
        {
            desired_image_count = display.d_surface_caps.max_image_count;
        }

        let transform = if display
            .d_surface_caps
            .supported_transforms
            .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
        {
            vk::SurfaceTransformFlagsKHR::IDENTITY
        } else {
            display.d_surface_caps.current_transform
        };

        // we need to check if the surface format supports the
        // storage image type
        let mut extra_usage = vk::ImageUsageFlags::empty();
        let mut swap_flags = vk::SwapchainCreateFlagsKHR::empty();
        let mut use_mut_swapchain = false;
        // We should use a mutable swapchain to allow for rendering to
        // RGBA8888 if the swapchain doesn't suppport it and if the mutable
        // swapchain extensions are present. This is for intel
        if display
            .d_surface_caps
            .supported_usage_flags
            .contains(vk::ImageUsageFlags::STORAGE)
        {
            extra_usage |= vk::ImageUsageFlags::STORAGE;
            log::info!(
                "Format {:?} supports Storage usage",
                display.d_surface_format.format
            );
        } else {
            assert!(dev_features.vkc_supports_mut_swapchain);
            log::info!(
                "Format {:?} does not support Storage usage, using mutable swapchain",
                display.d_surface_format.format
            );
            use_mut_swapchain = true;

            extra_usage |= vk::ImageUsageFlags::STORAGE;
            swap_flags |= vk::SwapchainCreateFlagsKHR::MUTABLE_FORMAT;
        }

        // see this for how to get storage swapchain on intel:
        // https://github.com/doitsujin/dxvk/issues/504

        let mut create_info = vk::SwapchainCreateInfoKHR::builder()
            .flags(swap_flags)
            .surface(display.d_surface)
            .min_image_count(desired_image_count)
            .image_color_space(display.d_surface_format.color_space)
            .image_format(display.d_surface_format.format)
            .image_extent(display.d_resolution)
            // the color attachment is guaranteed to be available
            //
            // WEIRD: validation layers throw an issue with this on intel since it doesn't
            // support storage for the swapchain format.
            // You can ignore this:
            // https://www.reddit.com/r/vulkan/comments/ahtw8x/shouldnt_validation_layers_catch_the_wrong_format/
            //
            // Leave the STORAGE flag to be explicit that we need it
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | extra_usage)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(display.d_present_mode)
            .clipped(true)
            .image_array_layers(1)
            .old_swapchain(match old_swapchain {
                Some(s) => s,
                None => vk::SwapchainKHR::null(),
            });

        if use_mut_swapchain {
            // specifying the mutable format flag also requires that we add a
            // list of additional formats. We need this so that mesa will
            // set VK_IMAGE_CREATE_EXTENDED_USAGE_BIT_KHR for the swapchain images
            // we also need to include the surface format, since it seems mesa wants
            // the supported format + any new formats we select.
            let add_formats = vk::ImageFormatListCreateInfoKHR::builder()
                // just add rgba32 because it's the most common.
                .view_formats(&[display.d_surface_format.format])
                .build();
            create_info.p_next = &add_formats as *const _ as *mut std::ffi::c_void;
        }

        // views for all of the swapchains images will be set up in
        // select_images_and_views
        swapchain_loader
            .create_swapchain(&create_info, None)
            .unwrap()
    }

    /// Get the vkImage's for the swapchain, and create vkImageViews for them
    ///
    /// get all the presentation images for the swapchain
    /// specify the image views, which specify how we want
    /// to access our images
    unsafe fn select_images_and_views(
        display: &mut Display,
        inst: &ash::Instance,
        swapchain_loader: &khr::Swapchain,
        swapchain: vk::SwapchainKHR,
        dev: &Device,
    ) -> (Vec<vk::Image>, Vec<vk::ImageView>) {
        let images = swapchain_loader.get_swapchain_images(swapchain).unwrap();

        let image_views = images
            .iter()
            .map(|&image| {
                let format_props = inst.get_physical_device_format_properties(
                    dev.pdev,
                    display.d_surface_format.format,
                );
                log::info!("format props: {:#?}", format_props);

                // we want to interact with this image as a 2D
                // array of RGBA pixels (i.e. the "normal" way)
                let mut create_info = vk::ImageViewCreateInfo::builder()
                    .view_type(vk::ImageViewType::TYPE_2D)
                    // see `create_swapchain` for why we don't use surface_format
                    .format(display.d_surface_format.format)
                    // select the normal RGBA type
                    // swap the R and B channels because we are mapping this
                    // to B8G8R8_SRGB using a mutable swapchain
                    // TODO: make mutable swapchain optional
                    .components(vk::ComponentMapping {
                        r: vk::ComponentSwizzle::R,
                        g: vk::ComponentSwizzle::G,
                        b: vk::ComponentSwizzle::B,
                        a: vk::ComponentSwizzle::A,
                    })
                    // this view pertains to the entire image
                    .subresource_range(vk::ImageSubresourceRange {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        base_mip_level: 0,
                        level_count: 1,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .image(image)
                    .build();

                let ext_info = vk::ImageViewUsageCreateInfoKHR::builder()
                    .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::STORAGE)
                    .build();

                // if the format doesn't support storage (intel doesn't),
                // then we need to attach an extra struct telling to to
                // allow the storage format in the view even though the
                // underlying format doesn't
                if !format_props
                    .optimal_tiling_features
                    .contains(vk::FormatFeatureFlags::STORAGE_IMAGE)
                {
                    create_info.p_next = &ext_info as *const _ as *mut std::ffi::c_void;
                }

                dev.dev.create_image_view(&create_info, None).unwrap()
            })
            .collect();

        return (images, image_views);
    }

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
        let size = [
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .build(),
            vk::DescriptorPoolSize::builder()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                // Okay it looks like this must match the layout
                // TODO: should this be changed?
                .descriptor_count(max_image_count)
                .build(),
        ];
        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);
        let bindless_pool = dev.dev.create_descriptor_pool(&info, None).unwrap();

        let bindings = [
            // the window list
            vk::DescriptorSetLayoutBinding::builder()
                .binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(
                    vk::ShaderStageFlags::COMPUTE
                        | vk::ShaderStageFlags::VERTEX
                        | vk::ShaderStageFlags::FRAGMENT,
                )
                .descriptor_count(1)
                .build(),
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
                vk::DescriptorBindingFlags::empty(), // the winlist
                Self::get_bindless_desc_flags(),     // The unbounded array of images
            ])
            .build();
        info.p_next = &usage_info as *const _ as *mut std::ffi::c_void;

        let bindless_layout = dev.dev.create_descriptor_set_layout(&info, None).unwrap();

        (bindless_pool, bindless_layout)
    }

    /// This helper ensures that our window list can hold `capacity` elements
    ///
    /// This will doube the winlist capacity until it fits.
    pub fn ensure_window_capacity(&mut self, capacity: usize) {
        if capacity >= self.r_windows_capacity {
            let mut new_capacity = 0;
            while new_capacity <= self.r_windows_capacity {
                new_capacity += self.r_windows_capacity;
            }

            unsafe {
                self.reallocate_windows_buf_with_cap(new_capacity);
            }
        }
    }

    /// This is a helper for reallocating the vulkan resources of the winlist
    unsafe fn reallocate_windows_buf_with_cap(&mut self, capacity: usize) {
        self.wait_for_prev_submit();

        self.dev.dev.destroy_buffer(self.r_windows_buf, None);
        self.dev.free_memory(self.r_windows_mem);

        // create our data and a storage buffer for the window list
        let (wl_storage, wl_storage_mem) = self.dev.create_buffer_with_size(
            vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::DEVICE_LOCAL
                | vk::MemoryPropertyFlags::HOST_VISIBLE
                | vk::MemoryPropertyFlags::HOST_COHERENT,
            (std::mem::size_of::<Window>() * capacity) as u64 + WINDOW_LIST_GLSL_OFFSET as u64,
        );
        self.dev
            .dev
            .bind_buffer_memory(wl_storage, wl_storage_mem, 0)
            .unwrap();
        self.r_windows_buf = wl_storage;
        self.r_windows_mem = wl_storage_mem;
        self.r_windows_capacity = capacity;
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
        info: &CreateInfo,
        ecs: &mut ll::Instance,
        mut img_ecs: ll::Instance,
        pass_comp: ll::Component<usize>,
    ) -> Result<(Renderer, Display)> {
        unsafe {
            let dev = Arc::new(Device::new(instance.clone(), info)?);

            // Our display is in charge of choosing a medium to draw on,
            // and will create a surface on that medium
            let mut display = Display::new(info, dev.clone());

            // TODO: allow for multiple pipes in use at once
            let pipe_type = if info.enable_traditional_composition {
                log::debug!("Using render pipeline");
                PipelineType::GEOMETRIC
            } else {
                panic!("Unsupported pipeline type");
            };

            // Each window is going to have a sampler descriptor for every
            // framebuffer image. Unfortunately this means the descriptor
            // count will be runtime dependent.
            // This is an allocator for those descriptors
            let descpool = DescPool::create(dev.clone());
            let sampler = dev.create_sampler();

            let swapchain_loader = khr::Swapchain::new(&instance.inst, &dev.dev);
            let swapchain = Renderer::create_swapchain(
                &mut display,
                &swapchain_loader,
                &dev.dev_features,
                pipe_type,
                None,
            );

            let (images, image_views) = Renderer::select_images_and_views(
                &mut display,
                &instance.inst,
                &swapchain_loader,
                swapchain,
                &dev,
            );

            let graphics_queue_family = Display::select_queue_family(
                &instance.inst,
                dev.pdev,
                &display.d_surface_loader,
                display.d_surface,
                vk::QueueFlags::GRAPHICS,
            );
            let present_queue = dev.dev.get_device_queue(graphics_queue_family, 0);

            let pool = dev.create_command_pool(graphics_queue_family);
            let buffers = dev.create_command_buffers(pool, images.len() as u32);

            let sema_create_info = vk::SemaphoreCreateInfo::default();

            let present_sema = dev.dev.create_semaphore(&sema_create_info, None).unwrap();
            let render_sema = dev.dev.create_semaphore(&sema_create_info, None).unwrap();

            let fence = dev
                .dev
                .create_fence(
                    &vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
                .expect("Could not create fence");

            let ext_mem_loader = khr::ExternalMemoryFd::new(&instance.inst, &dev.dev);

            let damage_regs = std::iter::repeat(Vec::new()).take(images.len()).collect();

            // TODO:
            // We need to handle the case where the ISV doesn't support
            // a large enough number of bound samplers. In that case, I guess
            // we need to do multiple instanced draw calls of the largest
            // size supported. This will only be doable with geom I guess
            // On moltenvk this is like 128, so that's bad
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
                display.d_surface_format.format,
                vk::ImageUsageFlags::SAMPLED,
                vk::ImageAspectFlags::COLOR,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::ImageTiling::LINEAR,
            );

            // Allocate our window order desc layout
            let bindings = [
                // the window order list
                vk::DescriptorSetLayoutBinding::builder()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .stage_flags(
                        vk::ShaderStageFlags::COMPUTE
                            | vk::ShaderStageFlags::VERTEX
                            | vk::ShaderStageFlags::FRAGMENT,
                    )
                    .descriptor_count(1)
                    .build(),
            ];
            let info = vk::DescriptorSetLayoutCreateInfo::builder().bindings(&bindings);
            let order_layout = dev.dev.create_descriptor_set_layout(&info, None).unwrap();

            // Create the window list component
            let win_comp = ecs.add_component();

            // Add our vulkan resource ECS entry
            let img_vk_comp = img_ecs.add_component();

            // Create the image vk info component
            // We have deleted this image, but it's invalid to pass a
            // NULL VkImageView as a descriptor. Instead we will populate
            // it with our "null"/"tmp" image, which is just a black square
            let null_sampler = sampler;
            let null_view = tmp_view;
            let mut img_info_comp = img_ecs.add_non_sparse_component(move || {
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
            let mut rend = Renderer {
                inst: instance,
                dev: dev,
                swapchain_loader: swapchain_loader,
                swapchain: swapchain,
                current_image: 0,
                fb_count: images.len(),
                swap_ages: std::iter::repeat(0).take(images.len()).collect(),
                damage_regions: damage_regs,
                current_damage: Vec::new(),
                surfacelist_regions: Vec::new(),
                images: images,
                image_sampler: sampler,
                views: image_views,
                graphics_queue_family: graphics_queue_family,
                r_present_queue: present_queue,
                pool: pool,
                cbufs: buffers,
                present_sema: present_sema,
                render_sema: render_sema,
                submit_fence: fence,
                external_mem_fd_loader: ext_mem_loader,
                r_release: Vec::new(),
                desc_pool: descpool,
                draw_call_submitted: false,
                r_pipe_type: pipe_type,
                r_ecs: ecs.clone(),
                r_images_desc_pool: bindless_pool,
                r_images_desc_layout: bindless_layout,
                r_images_desc: bindless_desc,
                r_images_desc_size: 0,
                r_windows: win_comp,
                r_windows_buf: vk::Buffer::null(),
                r_windows_mem: vk::DeviceMemory::null(),
                r_windows_capacity: 8,
                r_order_desc_layout: order_layout,
                tmp_image: tmp,
                tmp_image_view: tmp_view,
                tmp_image_mem: tmp_mem,
                r_barriers: VkBarriers {
                    r_acquire_barriers: Vec::new(),
                    r_release_barriers: Vec::new(),
                },
                r_null_image: null_image,
                r_image_ecs: img_ecs,
                r_image_vk: img_vk_comp,
                r_image_infos: img_info_comp,
                r_surface_pass: pass_comp,
            };
            rend.reallocate_windows_buf_with_cap(rend.r_windows_capacity);

            return Ok((rend, display));
        }
    }

    /// Recursively update the shader window parameters for surf
    ///
    /// This is used to push all CPU-side thundr data to the GPU for the shader
    /// to ork with. The offset is used through this to calculate the position of
    /// the subsurfaces relative to their parent.
    /// The flush argument forces the surface's data to be written back.
    fn update_window_list_recurse(
        &mut self,
        list: &mut SurfaceList,
        mut surf: Surface,
        offset: (i32, i32),
        flush: bool,
    ) {
        {
            // Only draw this surface if it has contents defined. Either
            // an image or a color
            //
            // Add this surface before its children, since we need to draw it
            // first so that any alpha in the children will see this underneath
            let internal = surf.s_internal.read().unwrap();
            if internal.s_image.is_some() || internal.s_color.is_some() {
                list.push_raw_order(self, &surf.get_ecs_id());
            }
        }

        if surf.modified() || flush {
            self.update_surf_shader_window(&surf, offset);
            surf.set_modified(false);
        }

        let surf_off = surf.get_pos();
        for i in 0..surf.get_subsurface_count() {
            let child = surf.get_subsurface(i);

            self.update_window_list_recurse(
                list,
                child,
                (offset.0 + surf_off.0 as i32, offset.1 + surf_off.1 as i32),
                // If the parent surface was moved, then we need to update all
                // children, since their positions are out of date.
                surf.modified() | flush,
            );
        }
    }

    /// Extract information for shaders from a surface list
    ///
    /// This includes dimensions, the image bound, etc.
    fn update_window_list(&mut self, surfaces: &mut SurfaceList) {
        surfaces.clear_order_buf();

        for i in (0..surfaces.len()).rev() {
            let s = surfaces[i as usize].clone();
            self.update_window_list_recurse(surfaces, s, (0, 0), false);
        }
    }

    /// Write our Thundr Surface's data to the window list we will pass to the shader
    ///
    /// The shader needs a contiguous list of surfaces, so we turn our surfaces
    /// into a bunch of "windows". These windows will have their size and offset
    /// populated, along with any other drawing data. These live in r_windows, and
    /// the order is set by the surfacelist's l_window_order.
    ///
    /// The offset parameter comes from the offset of this window due to its
    /// surface being a subsurface.
    fn update_surf_shader_window(&mut self, surf_rc: &Surface, offset: (i32, i32)) {
        // Our iterator is going to take into account the dimensions of the
        // parent surface(s), and give us the offset from which we should start
        // doing our calculations. Basically off_x is the parent surfaces X position.
        let surf = surf_rc.s_internal.read().unwrap();
        let opaque_reg = match surf_rc.get_opaque(self) {
            Some(r) => r,
            // If no opaque data was attached, place a -1 in the start.x component
            // to tell the shader to ignore this
            None => Rect::new(-1, 0, -1, 0),
        };
        let image_id = match surf.s_image.as_ref() {
            Some(i) => i.get_id().get_raw_id() as i32,
            None => -1,
        };

        self.r_windows.set(
            &surf_rc.s_window_id,
            Window {
                w_id: image_id,
                w_use_color: surf.s_color.is_some() as i32,
                w_pass: 0,
                w_padding: 0,
                w_color: match surf.s_color {
                    Some((r, g, b, a)) => (r, g, b, a),
                    // magic value so it's easy to debug
                    // this is clear, since we don't have a color
                    // assigned and we may not have an image bound.
                    // In that case, we want this surface to be clear.
                    None => (0.0, 50.0, 100.0, 0.0),
                },
                w_dims: Rect::new(
                    offset.0 + surf.s_rect.r_pos.0 as i32,
                    offset.1 + surf.s_rect.r_pos.1 as i32,
                    surf.s_rect.r_size.0 as i32,
                    surf.s_rect.r_size.1 as i32,
                ),
                w_opaque: opaque_reg,
            },
        );
    }

    /// Helper for getting the push constants
    ///
    /// This will be where we calculate the viewport scroll amount
    pub fn get_push_constants(
        &mut self,
        params: &RecordParams,
        viewport: &Viewport,
    ) -> PushConstants {
        // transform from blender's coordinate system to vulkan
        PushConstants {
            scroll_x: viewport.scroll_offset.0 as f32,
            scroll_y: viewport.scroll_offset.1 as f32,
            width: viewport.size.0 as f32,
            height: viewport.size.1 as f32,
            starting_depth: params.starting_depth,
        }
    }

    /// Wait for the submit_fence
    pub fn wait_for_prev_submit(&self) {
        self.dev.wait_for_copy();

        unsafe {
            match self.dev.dev.wait_for_fences(
                &[self.submit_fence],
                true,          // wait for all
                std::u64::MAX, //timeout
            ) {
                Ok(_) => {}
                Err(e) => match e {
                    vk::Result::ERROR_DEVICE_LOST => {
                        // If aftermath support is enabled, wait for aftermath
                        // to dump the GPU state
                        #[cfg(feature = "aftermath")]
                        {
                            self.inst.aftermath.wait_for_dump();
                        }
                    }
                    _ => panic!("Could not wait for vulkan fences"),
                },
            };
        }
    }

    pub fn get_recording_parameters(&mut self) -> RecordParams {
        RecordParams {
            cbuf: self.cbufs[self.current_image as usize],
            image_num: self.current_image as usize,
            // Start at max depth of 1.0 and go to zero
            starting_depth: 0.0,
        }
    }

    /// Adds damage to `regions` without modifying the damage
    fn aggregate_damage(&mut self, damage: &Damage) {
        for region in damage.regions() {
            let rect = vk::RectLayerKHR::builder()
                .offset(
                    vk::Offset2D::builder()
                        .x(region.r_pos.0)
                        .y(region.r_pos.1)
                        .build(),
                )
                .extent(
                    vk::Extent2D::builder()
                        .width(region.r_size.0 as u32)
                        .height(region.r_size.1 as u32)
                        .build(),
                )
                .build();

            self.surfacelist_regions.push(rect);
        }
    }

    /// Start recording a cbuf for one frame
    ///
    /// Each framebuffer has a set of resources, including command
    /// buffers. This records the cbufs for the framebuffer
    /// specified by `img`.
    ///
    /// The frame is not submitted to be drawn until
    /// `begin_frame` is called. `end_recording_one_frame` must be called
    /// before `begin_frame`
    ///
    /// This adds to the current_damage that has been set by surface moving
    /// and mapping.
    pub fn begin_recording_one_frame(&mut self) -> Result<RecordParams> {
        // At least wait for any image copies to complete
        self.dev.wait_for_copy();
        // get the next frame to draw into
        self.get_next_swapchain_image()?;

        // Now combine the first n lists (depending on the current
        // image's age) into one list for vkPresentRegionsKHR (and `gen_tile_list`)
        // We need to do this first since popping an entry off damage_regions
        // would remove one of the regions we need to process.
        // Using in lets us never go past the end of the array
        if self.dev.dev_features.vkc_supports_incremental_present {
            assert!(self.swap_ages[self.current_image as usize] <= self.damage_regions.len());
            for i in 0..(self.swap_ages[self.current_image as usize]) {
                self.current_damage.extend(&self.damage_regions[i as usize]);
            }

            // We need to accumulate a list of damage for the current frame. We are
            // going to retire the oldest damage lists, and create a new one from
            // the damages passed to surfaces
            let mut am_eldest = true;
            let mut next_oldest = 0;
            for (i, age) in self.swap_ages.iter().enumerate() {
                // oldest until proven otherwise
                if self.swap_ages[i] > self.swap_ages[self.current_image as usize] {
                    am_eldest = false;
                }
                // Get the max age of the other framebuffers
                if i != self.current_image as usize && *age > next_oldest {
                    next_oldest = *age;
                }
            }
            if am_eldest {
                log::debug!(
                    "I (image {:?}) am the eldest: {:?}",
                    self.current_image,
                    self.swap_ages
                );
                log::debug!(
                    "Truncating damage_regions from {:?} to {:?}",
                    self.damage_regions.len(),
                    next_oldest
                );
                self.damage_regions.truncate(next_oldest);
            }
        }

        Ok(self.get_recording_parameters())
    }

    pub fn add_damage_for_list(&mut self, surfaces: &mut SurfaceList) -> Result<()> {
        for surf_rc in surfaces.iter_mut() {
            // add the new damage to the list of damages
            // If the surface does not have damage attached, then don't generate tiles
            if let Some(damage) = surf_rc.get_global_damage(self) {
                self.aggregate_damage(&damage);
            }

            // now we have to consider damage caused by moving the surface
            //
            // We don't have to correct the position based on the surface pos
            // since the damage was already recorded for the surface
            if let Some(damage) = surf_rc.take_surface_damage() {
                self.aggregate_damage(&damage);
            }
        }

        // Finally we add any damage that the surfacelist has
        for damage in surfaces.damage() {
            self.aggregate_damage(damage);
        }
        surfaces.clear_damage();

        Ok(())
    }

    /// End a total frame recording
    ///
    /// This finalizes any damage and updates the buffer ages
    pub fn end_recording_one_frame(&mut self) {
        self.current_damage.extend(&self.surfacelist_regions);
        let mut regions = Vec::new();
        std::mem::swap(&mut regions, &mut self.surfacelist_regions);
        self.damage_regions.push_front(regions);

        // Only update the ages after we have processed them
        self.update_buffer_ages();
    }

    /// Allocate a descriptor set for each layout in `layouts`
    ///
    /// A descriptor set specifies a group of attachments that can
    /// be referenced by the graphics pipeline. Think of a descriptor
    /// as the hardware's handle to a resource. The set of descriptors
    /// allocated in each set is specified in the layout.
    pub(crate) unsafe fn allocate_descriptor_sets(
        &self,
        pool: vk::DescriptorPool,
        layouts: &[vk::DescriptorSetLayout],
    ) -> Vec<vk::DescriptorSet> {
        let info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(layouts)
            .build();

        self.dev.dev.allocate_descriptor_sets(&info).unwrap()
    }

    /// Update an image sampler descriptor set
    ///
    /// This is what actually sets the image that the sampler
    /// will filter for the shader. The image is referenced
    /// by the `view` argument.
    pub(crate) unsafe fn update_sampler_descriptor_set(
        &self,
        set: vk::DescriptorSet,
        binding: u32,
        element: u32,
        sampler: vk::Sampler,
        view: vk::ImageView,
    ) {
        let info = vk::DescriptorImageInfo::builder()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(view)
            .sampler(sampler)
            .build();
        let write_info = [vk::WriteDescriptorSet::builder()
            .dst_set(set)
            .dst_binding(binding)
            // descriptors can be arrays, so we need to specify an offset
            // into that array if applicable
            .dst_array_element(element)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&[info])
            .build()];

        self.dev.dev.update_descriptor_sets(
            &write_info, // descriptor writes
            &[],         // descriptor copies
        );
    }

    /// Create descriptors for the image samplers
    ///
    /// Each Image will have a descriptor for each framebuffer,
    /// since multiple frames will be in flight. This allocates
    /// `image_count` sampler descriptors.
    unsafe fn create_sampler_descriptors(
        &self,
        pool: vk::DescriptorPool,
        layout: vk::DescriptorSetLayout,
        image_count: u32,
    ) -> (vk::Sampler, Vec<vk::DescriptorSet>) {
        // One image sampler is going to be used for everything
        let sampler = self.dev.create_sampler();
        // A descriptor needs to be created for every swapchaing image
        // so we can prepare the next frame while the current one is
        // processing.
        let mut descriptors = Vec::new();

        for _ in 0..image_count {
            let set = self.allocate_descriptor_sets(pool, &[layout])[0];

            descriptors.push(set);
        }

        return (sampler, descriptors);
    }

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

    pub fn refresh_window_resources(&mut self, surfaces: &mut SurfaceList) {
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

        // Now that we have possibly reallocated the descriptor sets,
        // refresh the window list to put it back in gpu mem
        self.refresh_window_list(surfaces);

        // Now write the new bindless descriptor
        let write_infos = &[
            vk::WriteDescriptorSet::builder()
                .dst_set(self.r_images_desc)
                .dst_binding(0)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&[vk::DescriptorBufferInfo::builder()
                    .buffer(self.r_windows_buf)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build()])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.r_images_desc)
                .dst_binding(1)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(self.r_image_infos.get_data_slice().data())
                .build(),
        ];
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

        // We also need to tell the surface list to update its window
        // order resource
        surfaces.allocate_order_desc(self);
    }

    /// This refreshes the renderer's internal variable size window
    /// list that will be used as part of the bindless shader code.
    pub fn refresh_window_list(&mut self, surfaces: &mut SurfaceList) {
        // Only do this if the surface list has changed and the shader needs a new
        // window ordering
        // The surfacelist ordering didn't change, but the individual
        // surfaces might have. We need to copy the new values for
        // any changed
        self.update_window_list(surfaces);
        let num_entities = self.r_ecs.capacity();
        self.ensure_window_capacity(num_entities);

        surfaces.update_window_order_buf(self);

        // TODO: don't even use CPU copies of the datastructs and perform
        // the tile/window updates in the mapped GPU memory
        // (requires benchmark)
        // Don't update vulkan memory unless we have more than one valid id.
        if self.r_ecs.num_entities() > 0 && num_entities > 0 {
            // Shader expects struct WindowList { int count; Window windows[] }
            self.dev
                .update_memory(self.r_windows_mem, 0, &[num_entities]);
            self.dev.update_memory_from_callback(
                self.r_windows_mem,
                WINDOW_LIST_GLSL_OFFSET,
                num_entities,
                |dst| {
                    // For each valid window entry, extract the Window
                    // type from the option so that we can write it to
                    // the Vulkan memory
                    for p in surfaces.l_pass.iter() {
                        if let Some(pass) = p {
                            for id in pass.p_window_order.iter() {
                                let i = id.get_raw_id();
                                let win = self.r_windows.get(&id).unwrap();
                                log::debug!("Winlist index {}: writing window {:?}", i, *win);
                                dst[i] = *win;
                            }
                        }
                    }
                },
            );
        }
    }

    /// Update self.current_image with the swapchain image to render to
    ///
    /// If the next image is not ready (i.e. if Vulkan returned NOT_READY or
    /// TIMEOUT), then this will loop on calling `vkAcquireNextImageKHR` until
    /// it gets a valid image. This has to be done on AMD hw or else the TIMEOUT
    /// error will get passed up the callstack and fail.
    pub fn get_next_swapchain_image(&mut self) -> Result<()> {
        unsafe {
            loop {
                match self.swapchain_loader.acquire_next_image(
                    self.swapchain,
                    0,                 // use a zero timeout to immediately get the state
                    self.present_sema, // signals presentation
                    vk::Fence::null(),
                ) {
                    // TODO: handle suboptimal surface regeneration
                    Ok((index, _)) => {
                        log::debug!(
                            "Getting next swapchain image: Current {:?}, New {:?}",
                            self.current_image,
                            index
                        );
                        self.current_image = index;
                        return Ok(());
                    }
                    Err(vk::Result::NOT_READY) => {
                        log::debug!(
                            "vkAcquireNextImageKHR: vk::Result::NOT_READY: Current {:?}",
                            self.current_image
                        );
                        continue;
                    }
                    Err(vk::Result::TIMEOUT) => {
                        log::debug!(
                            "vkAcquireNextImageKHR: vk::Result::TIMEOUT: Current {:?}",
                            self.current_image
                        );
                        continue;
                    }
                    Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Err(ThundrError::OUT_OF_DATE),
                    Err(vk::Result::SUBOPTIMAL_KHR) => return Err(ThundrError::OUT_OF_DATE),
                    // the call did not succeed
                    Err(_) => return Err(ThundrError::COULD_NOT_ACQUIRE_NEXT_IMAGE),
                }
            }
        }
    }

    /// This increments the ages of all buffers, except current_image.
    /// The current_image is reset to 0 since it is in use.
    fn update_buffer_ages(&mut self) {
        for (i, age) in self.swap_ages.iter_mut().enumerate() {
            if i != self.current_image as usize {
                *age += 1;
            }
        }
        self.swap_ages[self.current_image as usize] = 0;
    }

    /// Returns true if we are ready to call present
    pub fn frame_submission_complete(&mut self) -> bool {
        match unsafe { self.dev.dev.get_fence_status(self.submit_fence) } {
            // true means vk::Result::SUCCESS
            // false means vk::Result::NOT_READY
            Ok(complete) => return complete,
            Err(_) => panic!("Failed to get fence status"),
        };
    }

    /// Present the current swapchain image to the screen.
    ///
    /// Finally we can actually flip the buffers and present
    /// this image.
    pub fn present(&mut self) -> Result<()> {
        // This is a bit odd. So if a draw call was submitted, then
        // we need to wait for rendering to complete before presenting. If
        // no draw call was submitted (no work to do) then we need to
        // wait on the present of the previous frame.
        let wait_semas = match self.draw_call_submitted {
            true => [self.render_sema],
            false => {
                panic!("No draw call was submitted, but thundr.present was still called");
            }
        };
        let swapchains = [self.swapchain];
        let indices = [self.current_image];
        let mut info = vk::PresentInfoKHR::builder()
            .wait_semaphores(&wait_semas)
            .swapchains(&swapchains)
            .image_indices(&indices);

        if self.dev.dev_features.vkc_supports_incremental_present {
            if self.current_damage.len() > 0 {
                let pres_info = vk::PresentRegionsKHR::builder()
                    .regions(&[vk::PresentRegionKHR::builder()
                        .rectangles(self.current_damage.as_slice())
                        .build()])
                    .build();
                info.p_next = &pres_info as *const _ as *const c_void;
            }
        }
        // Now that this frame's damage has been consumed, clear it
        self.current_damage.clear();
        self.surfacelist_regions.clear();

        unsafe {
            match self
                .swapchain_loader
                .queue_present(self.r_present_queue, &info)
            {
                Ok(_) => Ok(()),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(ThundrError::OUT_OF_DATE),
                Err(vk::Result::SUBOPTIMAL_KHR) => Err(ThundrError::OUT_OF_DATE),
                Err(_) => Err(ThundrError::PRESENT_FAILED),
            }
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

            self.dev.dev.destroy_semaphore(self.present_sema, None);
            self.dev.dev.destroy_semaphore(self.render_sema, None);
            self.dev.dev.destroy_sampler(self.image_sampler, None);
            self.dev
                .dev
                .destroy_descriptor_set_layout(self.r_images_desc_layout, None);

            self.dev
                .dev
                .destroy_descriptor_set_layout(self.r_order_desc_layout, None);
            self.dev
                .dev
                .destroy_descriptor_pool(self.r_images_desc_pool, None);
            self.dev.dev.destroy_buffer(self.r_windows_buf, None);
            self.dev.free_memory(self.r_windows_mem);

            self.destroy_swapchain();

            self.dev.dev.destroy_command_pool(self.pool, None);
            self.dev.dev.destroy_fence(self.submit_fence, None);
        }
    }
}
