// A vulkan rendering backend
//
// This layer is very low, and as a result is mostly unsafe. Nothing
// unsafe/vulkan/ash/etc should be exposed to upper layers
//
// Austin Shafer - 2020
#![allow(dead_code, non_camel_case_types)]
use std::collections::VecDeque;
use std::ffi::{CStr, CString};
use std::marker::Copy;
use std::os::raw::c_void;

use ash::extensions::ext;
use ash::extensions::khr;
use ash::{vk, Device, Entry, Instance};

use crate::descpool::DescPool;
use crate::display::Display;
use crate::list::SurfaceList;
use crate::pipelines::PipelineType;
use crate::platform::VKDeviceFeatures;
use crate::{Image, Surface};

use serde::{Deserialize, Serialize};

extern crate utils as cat5_utils;
use crate::{CreateInfo, Damage};
use crate::{Result, ThundrError};
use cat5_utils::{log, region::Rect, MemImage};

// Nvidia aftermath SDK GPU crashdump support
#[cfg(feature = "aftermath")]
extern crate nvidia_aftermath_rs as aftermath;
#[cfg(feature = "aftermath")]
use aftermath::Aftermath;

use utils::ecs::{ECSId, ECSInstance, ECSTable};

/// This is the offset from the base of the winlist buffer to the
/// window array in the actual ssbo. This needs to match the `offset`
/// field in the `layout` qualifier in the shaders
const WINDOW_LIST_GLSL_OFFSET: isize = 16;

// this happy little debug callback is from the ash examples
// all it does is print any errors/warnings thrown.
unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_types: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> u32 {
    log::error!(
        "[VK][{:?}][{:?}] {:?}",
        message_severity,
        message_types,
        CStr::from_ptr(p_callback_data.as_ref().unwrap().p_message)
    );
    println!();
    vk::FALSE
}

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
    /// debug callback sugar mentioned earlier
    debug_loader: ext::DebugUtils,
    debug_callback: vk::DebugUtilsMessengerEXT,

    /// the entry just loads function pointers from the dynamic library
    /// I am calling it a loader, because that's what it does
    pub(crate) loader: Entry,
    /// the big vulkan instance.
    pub(crate) inst: Instance,
    /// the logical device we are using
    /// maybe I'll test around with multi-gpu
    pub(crate) dev: Device,
    pub(crate) dev_features: VKDeviceFeatures,
    /// the physical device selected to display to
    pub(crate) pdev: vk::PhysicalDevice,
    pub(crate) mem_props: vk::PhysicalDeviceMemoryProperties,

    /// index into the array of queue families
    pub(crate) graphics_family_index: u32,
    pub(crate) transfer_family_index: u32,
    /// processes things to be physically displayed
    pub(crate) present_queue: vk::Queue,
    /// queue for copy operations
    pub(crate) transfer_queue: vk::Queue,

    /// vk_khr_display and vk_khr_surface wrapper.
    pub(crate) display: Display,
    pub(crate) surface_format: vk::SurfaceFormatKHR,
    pub(crate) surface_caps: vk::SurfaceCapabilitiesKHR,
    /// resolution to create the swapchain with
    pub(crate) resolution: vk::Extent2D,

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
    /// This is the final compiled set of damages for this frame.
    pub(crate) current_damage: Vec<vk::RectLayerKHR>,

    // TODO: move cbuf management from Renderer to the pipelines
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
    pub(crate) r_release: Vec<Box<dyn Drop>>,
    /// command buffer for copying shm images
    pub(crate) copy_cbuf: vk::CommandBuffer,
    pub(crate) copy_cbuf_fence: vk::Fence,
    /// This is an allocator for the dynamic sets (samplers)
    pub(crate) desc_pool: DescPool,

    /// These are for loading textures into images
    pub(crate) transfer_buf_len: usize,
    pub(crate) transfer_buf: vk::Buffer,
    pub(crate) transfer_mem: vk::DeviceMemory,

    /// Has vkQueueSubmit been called.
    pub(crate) draw_call_submitted: bool,

    /// The type of pipeline(s) being in use
    pub(crate) r_pipe_type: PipelineType,

    /// We keep a list of image views from the surface list's images
    /// to be passed as our unsized image array in our shader. This needs
    /// to be regenerated any time a change to the surfacelist is made
    pub(crate) r_image_infos: Vec<vk::DescriptorImageInfo>,
    pub(crate) r_images_desc_pool: vk::DescriptorPool,
    pub(crate) r_images_desc_layout: vk::DescriptorSetLayout,
    pub(crate) r_images_desc: vk::DescriptorSet,

    /// The list of window dimensions that is passed to the shader
    pub r_windows: ECSTable<Window>,
    pub r_windows_buf: vk::Buffer,
    pub r_windows_mem: vk::DeviceMemory,
    /// The number of Windows that r_winlist_mem was allocate to hold
    pub r_windows_capacity: usize,
    /// The order of windows to be drawn. References r_windows.
    ///
    /// This is sorted back to front, where back comes first. i.e. the
    /// things you want to draw first should be in front of things that
    /// you want to be able to blend overtop of.
    pub r_window_order: Vec<ECSId>,
    pub r_order_buf: vk::Buffer,
    pub r_order_mem: vk::DeviceMemory,
    pub r_order_capacity: usize,

    /// Temporary image to bind to the image list when
    /// no images are attached.
    tmp_image: vk::Image,
    tmp_image_view: vk::ImageView,
    tmp_image_mem: vk::DeviceMemory,

    /// Dmabuf import usage barrier list. Will be regenerated
    /// during every draw
    pub r_acquire_barriers: Vec<vk::ImageMemoryBarrier>,
    /// Dmabuf import release barriers. These let drm know vulkan
    /// is done using them.
    pub r_release_barriers: Vec<vk::ImageMemoryBarrier>,

    /// Nvidia Aftermath SDK instance. Inclusion of this enables
    /// GPU crashdumps.
    #[cfg(feature = "aftermath")]
    r_aftermath: Aftermath,
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
    /// w_id.0: id that's the offset into the unbound sampler array
    /// w_id.1: if we should use w_color instead of texturing
    pub w_id: (i32, i32, i32, i32),
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
}

// Most of the functions below will be unsafe. Only the safe functions
// should be used by the applications. The unsafe functions are mostly for
// internal use.
impl Renderer {
    /// Creates a new debug reporter and registers our function
    /// for debug callbacks so we get nice error messages
    unsafe fn setup_debug(
        entry: &Entry,
        instance: &Instance,
    ) -> (ext::DebugUtils, vk::DebugUtilsMessengerEXT) {
        let debug_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                    | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
            )
            .pfn_user_callback(Some(vulkan_debug_callback));

        let dr_loader = ext::DebugUtils::new(entry, instance);
        let callback = dr_loader
            .create_debug_utils_messenger(&debug_info, None)
            .unwrap();
        return (dr_loader, callback);
    }

    /// Create a vkInstance
    ///
    /// Most of the create info entries are straightforward, with
    /// some basic extensions being enabled. All of the work is
    /// done in subfunctions.
    unsafe fn create_instance(info: &CreateInfo) -> (Entry, Instance) {
        let entry = Entry::linked();
        let app_name = CString::new("Thundr").unwrap();

        // For some reason old versions of the validation layers segfault in renderpass on the
        // geometric one, so only use validation on compute
        let layer_names = vec![
            #[cfg(debug_assertions)]
            CString::new("VK_LAYER_KHRONOS_validation").unwrap(),
            #[cfg(target_os = "macos")]
            CString::new("VK_LAYER_KHRONOS_synchronization2").unwrap(),
        ];

        let layer_names_raw: Vec<*const i8> = layer_names
            .iter()
            .map(|raw_name: &CString| raw_name.as_ptr())
            .collect();

        let mut extension_names_raw = Display::extension_names(info);
        extension_names_raw.push(ext::DebugUtils::name().as_ptr());

        let appinfo = vk::ApplicationInfo::builder()
            .application_name(&app_name)
            .application_version(0)
            .engine_name(&app_name)
            .engine_version(0)
            .api_version(vk::API_VERSION_1_2);

        let mut create_info = vk::InstanceCreateInfo::builder()
            .application_info(&appinfo)
            .enabled_layer_names(&layer_names_raw)
            .enabled_extension_names(&extension_names_raw);

        let printf_info = vk::ValidationFeaturesEXT::builder()
            .enabled_validation_features(&[vk::ValidationFeatureEnableEXT::DEBUG_PRINTF])
            .build();
        create_info.p_next = &printf_info as *const _ as *const std::os::raw::c_void;

        let instance: Instance = entry
            .create_instance(&create_info, None)
            .expect("Instance creation error");

        return (entry, instance);
    }

    /// Check if a queue family is suited for our needs.
    /// Queue families need to support graphical presentation and
    /// presentation on the given surface.
    unsafe fn is_valid_queue_family(
        pdevice: vk::PhysicalDevice,
        info: vk::QueueFamilyProperties,
        index: u32,
        surface_loader: &khr::Surface,
        surface: vk::SurfaceKHR,
        flags: vk::QueueFlags,
    ) -> bool {
        info.queue_flags.contains(flags)
            && surface_loader
                // ensure compatibility with the surface
                .get_physical_device_surface_support(pdevice, index, surface)
                .unwrap()
    }

    /// Get the major/minor of the DRM node in use
    ///
    /// This uses VK_EXT_physical_device_drm, and will fail an assert
    /// if it is not in use.
    ///
    /// return is drm (renderMajor, renderMinor).
    pub unsafe fn get_drm_dev(&self) -> (i64, i64) {
        if !self.dev_features.vkc_supports_phys_dev_drm {
            log::error!("Using drm Vulkan extensions but the underlying vulkan library doesn't support them. This will cause problems");
        }
        let mut drm_info = vk::PhysicalDeviceDrmPropertiesEXT::builder().build();

        let mut info = vk::PhysicalDeviceProperties2::builder().build();
        info.p_next = &mut drm_info as *mut _ as *mut std::ffi::c_void;

        self.inst
            .get_physical_device_properties2(self.pdev, &mut info);
        assert!(drm_info.has_render != 0);

        (drm_info.render_major, drm_info.render_minor)
    }

    /// Choose a vkPhysicalDevice and queue family index.
    ///
    /// selects a physical device and a queue family
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface.
    unsafe fn select_pdev(inst: &Instance) -> vk::PhysicalDevice {
        let pdevices = inst
            .enumerate_physical_devices()
            .expect("Physical device error");

        // for each physical device
        *pdevices
            .iter()
            // eventually there needs to be a way of grabbing
            // the configured pdev from the user
            .nth(0)
            // for now we are just going to get the first one
            .expect("Couldn't find suitable device.")
    }

    /// Choose a queue family
    ///
    /// returns an index into the array of queue types.
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface
    pub unsafe fn select_queue_family(
        inst: &Instance,
        pdev: vk::PhysicalDevice,
        surface_loader: &khr::Surface,
        surface: vk::SurfaceKHR,
        flags: vk::QueueFlags,
    ) -> u32 {
        // get the properties per queue family
        inst.get_physical_device_queue_family_properties(pdev)
            // for each property info
            .iter()
            .enumerate()
            .filter_map(|(index, info)| {
                // add the device and the family to a list of
                // candidates for use later
                match Renderer::is_valid_queue_family(
                    pdev,
                    *info,
                    index as u32,
                    surface_loader,
                    surface,
                    flags,
                ) {
                    // return the pdevice/family pair
                    true => Some(index as u32),
                    false => None,
                }
            })
            .nth(0)
            .expect("Could not find a suitable queue family")
    }

    /// get the vkPhysicalDeviceMemoryProperties structure for a vkPhysicalDevice
    pub(crate) unsafe fn get_pdev_mem_properties(
        inst: &Instance,
        pdev: vk::PhysicalDevice,
    ) -> vk::PhysicalDeviceMemoryProperties {
        inst.get_physical_device_memory_properties(pdev)
    }

    /// Create a vkDevice from a vkPhysicalDevice
    ///
    /// Create a logical device for interfacing with the physical device.
    /// once again we specify any device extensions we need, the swapchain
    /// being the most important one.
    ///
    /// A queue is created in the specified queue family in the
    /// present_queue argument.
    unsafe fn create_device(
        dev_features: &VKDeviceFeatures,
        inst: &Instance,
        pdev: vk::PhysicalDevice,
        queues: &[u32],
    ) -> Device {
        let dev_extension_names = dev_features.get_device_extensions();

        let features = vk::PhysicalDeviceFeatures::builder()
            .shader_clip_distance(true)
            .vertex_pipeline_stores_and_atomics(true)
            .fragment_stores_and_atomics(true)
            .build();

        // for now we only have one graphics queue, so one priority
        let priorities = [1.0];
        let mut queue_infos = Vec::new();
        for i in queues {
            queue_infos.push(
                vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(*i)
                    .queue_priorities(&priorities)
                    .build(),
            );
        }

        let mut dev_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(queue_infos.as_ref())
            .enabled_extension_names(dev_extension_names.as_slice())
            .enabled_features(&features)
            .build();

        if dev_features.vkc_supports_desc_indexing {
            let indexing_info = vk::PhysicalDeviceDescriptorIndexingFeaturesEXT::builder()
                .shader_sampled_image_array_non_uniform_indexing(true)
                .runtime_descriptor_array(true)
                .descriptor_binding_variable_descriptor_count(true)
                .descriptor_binding_partially_bound(true)
                .descriptor_binding_update_unused_while_pending(true)
                .build();

            dev_create_info.p_next = &indexing_info as *const _ as *mut std::ffi::c_void;
        }

        #[cfg(feature = "aftermath")]
        {
            let mut aftermath_info = vk::DeviceDiagnosticsConfigCreateInfoNV::builder()
                .flags(
                    vk::DeviceDiagnosticsConfigFlagsNV::ENABLE_SHADER_DEBUG_INFO
                        | vk::DeviceDiagnosticsConfigFlagsNV::ENABLE_RESOURCE_TRACKING,
                )
                .build();
            aftermath_info.p_next = dev_create_info.p_next;

            dev_create_info.p_next = &aftermath_info as *const _ as *mut std::ffi::c_void;
            // do our call here so aftermath_info is still in scope
            return inst.create_device(pdev, &dev_create_info, None).unwrap();
        }

        // return a newly created device
        inst.create_device(pdev, &dev_create_info, None).unwrap()
    }

    /// Tear down all the swapchain-dependent vulkan objects we have created.
    /// This will be used when dropping everything and when we need to handle
    /// OOD events.
    unsafe fn destroy_swapchain(&mut self) {
        // Don't destroy the images here, the destroy swapchain call
        // will take care of them
        for view in self.views.iter() {
            self.dev.destroy_image_view(*view, None);
        }
        self.views.clear();

        self.dev
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
    pub unsafe fn recreate_swapchain(&mut self) {
        // first wait for the device to finish working
        self.dev.device_wait_idle().unwrap();
        self.destroy_swapchain();

        // We need to get the updated size of our swapchain. This
        // will be the current size of the surface in use. We should
        // also update Display.d_resolution while we are at it.
        let new_res = self.display.get_vulkan_drawable_size(self.pdev);
        self.display.d_resolution = new_res;
        self.resolution = new_res;

        self.swapchain = Renderer::create_swapchain(
            &self.inst,
            &self.swapchain_loader,
            &self.display.d_surface_loader,
            self.pdev,
            self.display.d_surface,
            &self.surface_caps,
            self.surface_format,
            &self.resolution,
            &self.dev_features,
            self.r_pipe_type,
        );

        let (images, views) = Renderer::select_images_and_views(
            &self.inst,
            self.pdev,
            &self.swapchain_loader,
            self.swapchain,
            &self.dev,
            self.surface_format,
        );
        self.images = images;
        self.views = views;

        self.cbufs =
            Renderer::create_command_buffers(&self.dev, self.pool, self.images.len() as u32);
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
        _inst: &Instance,
        swapchain_loader: &khr::Swapchain,
        surface_loader: &khr::Surface,
        pdev: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        surface_caps: &vk::SurfaceCapabilitiesKHR,
        surface_format: vk::SurfaceFormatKHR,
        resolution: &vk::Extent2D,
        dev_features: &VKDeviceFeatures,
        _pipe_type: PipelineType,
    ) -> vk::SwapchainKHR {
        // how many images we want the swapchain to contain
        let mut desired_image_count = surface_caps.min_image_count + 1;
        if surface_caps.max_image_count > 0 && desired_image_count > surface_caps.max_image_count {
            desired_image_count = surface_caps.max_image_count;
        }

        let transform = if surface_caps
            .supported_transforms
            .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
        {
            vk::SurfaceTransformFlagsKHR::IDENTITY
        } else {
            surface_caps.current_transform
        };

        // the best mode for presentation is FIFO (with triple buffering)
        // as this is recommended by the samsung developer page, which
        // I am *assuming* is a good reference for low power apps
        let present_modes = surface_loader
            .get_physical_device_surface_present_modes(pdev, surface)
            .unwrap();
        let mode = present_modes
            .iter()
            .cloned()
            .find(|&mode| mode == vk::PresentModeKHR::FIFO)
            // fallback to FIFO if the mailbox mode is not available
            .unwrap_or(vk::PresentModeKHR::FIFO);

        // we need to check if the surface format supports the
        // storage image type
        let mut extra_usage = vk::ImageUsageFlags::empty();
        let mut swap_flags = vk::SwapchainCreateFlagsKHR::empty();
        let mut use_mut_swapchain = false;
        // We should use a mutable swapchain to allow for rendering to
        // RGBA8888 if the swapchain doesn't suppport it and if the mutable
        // swapchain extensions are present. This is for intel
        if surface_caps
            .supported_usage_flags
            .contains(vk::ImageUsageFlags::STORAGE)
        {
            extra_usage |= vk::ImageUsageFlags::STORAGE;
            log::debug!("Format {:?} supports Storage usage", surface_format.format);
        } else {
            assert!(dev_features.vkc_supports_mut_swapchain);
            log::debug!(
                "Format {:?} does not support Storage usage, using mutable swapchain",
                surface_format.format
            );
            use_mut_swapchain = true;

            extra_usage |= vk::ImageUsageFlags::STORAGE;
            swap_flags |= vk::SwapchainCreateFlagsKHR::MUTABLE_FORMAT;
        }

        // see this for how to get storage swapchain on intel:
        // https://github.com/doitsujin/dxvk/issues/504

        let mut create_info = vk::SwapchainCreateInfoKHR::builder()
            .flags(swap_flags)
            .surface(surface)
            .min_image_count(desired_image_count)
            .image_color_space(surface_format.color_space)
            .image_format(surface_format.format)
            .image_extent(*resolution)
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
            .present_mode(mode)
            .clipped(true)
            .image_array_layers(1);

        if use_mut_swapchain {
            // specifying the mutable format flag also requires that we add a
            // list of additional formats. We need this so that mesa will
            // set VK_IMAGE_CREATE_EXTENDED_USAGE_BIT_KHR for the swapchain images
            // we also need to include the surface format, since it seems mesa wants
            // the supported format + any new formats we select.
            let add_formats = vk::ImageFormatListCreateInfoKHR::builder()
                // just add rgba32 because it's the most common.
                .view_formats(&[surface_format.format])
                .build();
            create_info.p_next = &add_formats as *const _ as *mut std::ffi::c_void;
        }

        // views for all of the swapchains images will be set up in
        // select_images_and_views
        swapchain_loader
            .create_swapchain(&create_info, None)
            .unwrap()
    }

    /// returns a new vkCommandPool
    ///
    /// Command buffers are allocated from command pools. That's about
    /// all they do. They just manage memory. Command buffers will be allocated
    /// as part of the queue_family specified.
    pub(crate) unsafe fn create_command_pool(dev: &Device, queue_family: u32) -> vk::CommandPool {
        let pool_create_info = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);

        dev.create_command_pool(&pool_create_info, None).unwrap()
    }

    /// Allocate a vec of vkCommandBuffers
    ///
    /// Command buffers are constructed once, and can be executed
    /// many times. They also have the added bonus of being added to
    /// by multiple threads. Command buffer is shortened to `cbuf` in
    /// many areas of the code.
    ///
    /// For now we are only allocating two: one to set up the resources
    /// and one to do all the work.
    pub(crate) unsafe fn create_command_buffers(
        dev: &Device,
        pool: vk::CommandPool,
        count: u32,
    ) -> Vec<vk::CommandBuffer> {
        let cbuf_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(count)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY);

        dev.allocate_command_buffers(&cbuf_allocate_info).unwrap()
    }

    /// Get the vkImage's for the swapchain, and create vkImageViews for them
    ///
    /// get all the presentation images for the swapchain
    /// specify the image views, which specify how we want
    /// to access our images
    unsafe fn select_images_and_views(
        inst: &Instance,
        pdev: vk::PhysicalDevice,
        swapchain_loader: &khr::Swapchain,
        swapchain: vk::SwapchainKHR,
        dev: &Device,
        surface_format: vk::SurfaceFormatKHR,
    ) -> (Vec<vk::Image>, Vec<vk::ImageView>) {
        let images = swapchain_loader.get_swapchain_images(swapchain).unwrap();

        let image_views = images
            .iter()
            .map(|&image| {
                let format_props =
                    inst.get_physical_device_format_properties(pdev, surface_format.format);
                log::debug!("format props: {:#?}", format_props);

                // we want to interact with this image as a 2D
                // array of RGBA pixels (i.e. the "normal" way)
                let mut create_info = vk::ImageViewCreateInfo::builder()
                    .view_type(vk::ImageViewType::TYPE_2D)
                    // see `create_swapchain` for why we don't use surface_format
                    .format(surface_format.format)
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

                dev.create_image_view(&create_info, None).unwrap()
            })
            .collect();

        return (images, image_views);
    }

    /// Returns an index into the array of memory types for the memory
    /// properties
    ///
    /// Memory types specify the location and accessability of memory. Device
    /// local memory is resident on the GPU, while host visible memory can be
    /// read from the system side. Both of these are part of the
    /// vk::MemoryPropertyFlags type.
    fn find_memory_type_index(
        props: &vk::PhysicalDeviceMemoryProperties,
        reqs: &vk::MemoryRequirements,
        flags: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        // for each memory type
        for (i, ref mem_type) in props.memory_types.iter().enumerate() {
            // Bit i of memoryBitTypes will be set if the resource supports
            // the ith memory type in props.
            //
            // ash autogenerates common operations for bitfield style structs
            // they can be found in `vk_bitflags_wrapped`
            if (reqs.memory_type_bits >> i) & 1 == 1 && mem_type.property_flags.contains(flags) {
                // log!(LogLevel::profiling, "Selected type with flags {:?}",
                //          mem_type.property_flags);
                // return the index into the memory type array
                return Some(i as u32);
            }
        }
        None
    }

    /// Create a vkImage and the resources needed to use it
    ///   (vkImageView and vkDeviceMemory)
    ///
    /// Images are generic buffers which can be used as sources or
    /// destinations of data. Images are accessed through image views,
    /// which specify how the image will be modified or read. In vulkan
    /// memory management is more hands on, so we will allocate some device
    /// memory to back the image.
    ///
    /// This method may require some adjustment as it makes some assumptions
    /// about the type of image to be created.
    ///
    /// Resolution should probably be the same size as the swapchain's images
    /// usage defines the role the image will serve (transfer, depth data, etc)
    /// flags defines the memory type (probably DEVICE_LOCAL + others)
    pub(crate) unsafe fn create_image(
        dev: &Device,
        mem_props: &vk::PhysicalDeviceMemoryProperties,
        resolution: &vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
        flags: vk::MemoryPropertyFlags,
        tiling: vk::ImageTiling,
    ) -> (vk::Image, vk::ImageView, vk::DeviceMemory) {
        // we create the image now, but will have to bind
        // some memory to it later.
        let create_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width: resolution.width,
                height: resolution.height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(tiling)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let image = dev.create_image(&create_info, None).unwrap();

        // we need to find a memory type that matches the type our
        // new image needs
        let mem_reqs = dev.get_image_memory_requirements(image);
        let memtype_index = Renderer::find_memory_type_index(mem_props, &mem_reqs, flags).unwrap();

        let alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(mem_reqs.size)
            .memory_type_index(memtype_index);

        let image_memory = dev.allocate_memory(&alloc_info, None).unwrap();
        dev.bind_image_memory(image, image_memory, 0)
            .expect("Unable to bind device memory to image");

        let view_info = vk::ImageViewCreateInfo::builder()
            .subresource_range(
                vk::ImageSubresourceRange::builder()
                    .aspect_mask(aspect)
                    .level_count(1)
                    .layer_count(1)
                    .build(),
            )
            .image(image)
            .format(create_info.format)
            .view_type(vk::ImageViewType::TYPE_2D);

        let view = dev.create_image_view(&view_info, None).unwrap();

        return (image, view, image_memory);
    }

    /// Create an image sampler for the swapchain fbs
    ///
    /// Samplers are used to filter data from an image when
    /// it is referenced from a fragment shader. It allows
    /// for additional processing effects on the input.
    pub(crate) unsafe fn create_sampler(dev: &Device) -> vk::Sampler {
        let info = vk::SamplerCreateInfo::builder()
            // filter for magnified (oversampled) pixels
            .mag_filter(vk::Filter::LINEAR)
            // filter for minified (undersampled) pixels
            .min_filter(vk::Filter::LINEAR)
            // don't repeat the texture on wraparound
            // There is some weird thing where one/two pixels on each border
            // will repeat, which makes text rendering borked. Idk why this
            // is the case, but given that it only affects the very edges just
            // turn off repeat since we will never be doing it anyway)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER)
            // disable this for performance
            .anisotropy_enable(false)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
            // texture coords are [0,1)
            .unnormalized_coordinates(false)
            .compare_enable(false)
            .compare_op(vk::CompareOp::ALWAYS)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR);

        dev.create_sampler(&info, None).unwrap()
    }

    /// Transitions `image` to the `new` layout using `cbuf`
    ///
    /// Images need to be manually transitioned from two layouts. A
    /// normal use case is transitioning an image from an undefined
    /// layout to the optimal shader access layout. This is also
    /// used  by depth images.
    ///
    /// It is assumed this is for textures referenced from the fragment
    /// shader, and so it is a bit specific.
    pub unsafe fn transition_image_layout(
        &self,
        image: vk::Image,
        cbuf: vk::CommandBuffer,
        old: vk::ImageLayout,
        new: vk::ImageLayout,
    ) {
        // use defaults here, and set them in the next section
        let mut layout_barrier = vk::ImageMemoryBarrier::builder()
            .image(image)
            .src_access_mask(vk::AccessFlags::TRANSFER_READ)
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            // go from an undefined old layout to whatever the
            // driver decides is the optimal depth layout
            .old_layout(old)
            .new_layout(new)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .subresource_range(
                vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1)
                    .level_count(1)
                    .build(),
            )
            .build();
        #[allow(unused_assignments)]
        let mut src_stage = vk::PipelineStageFlags::TOP_OF_PIPE;
        #[allow(unused_assignments)]
        let mut dst_stage = vk::PipelineStageFlags::TOP_OF_PIPE;

        // automatically detect the pipeline src/dest stages to use.
        // straight from `transitionImageLayout` in the tutorial.
        if old == vk::ImageLayout::UNDEFINED {
            layout_barrier.src_access_mask = vk::AccessFlags::default();
            layout_barrier.dst_access_mask = vk::AccessFlags::TRANSFER_WRITE;

            src_stage = vk::PipelineStageFlags::TOP_OF_PIPE;
            dst_stage = vk::PipelineStageFlags::TRANSFER;
        } else {
            layout_barrier.src_access_mask = vk::AccessFlags::TRANSFER_WRITE;
            layout_barrier.dst_access_mask = vk::AccessFlags::SHADER_READ;

            src_stage = vk::PipelineStageFlags::TRANSFER;
            dst_stage = vk::PipelineStageFlags::FRAGMENT_SHADER;
        }

        // process the barrier we created, which will perform
        // the actual transition.
        self.dev.cmd_pipeline_barrier(
            cbuf,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[layout_barrier],
        );
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
    pub fn register_for_release(&mut self, release: Box<dyn Drop>) {
        self.r_release.push(release);
    }

    /// Update an image from a VkBuffer
    ///
    /// It is common to copy host data into an image
    /// to initialize it. This function initializes
    /// image by copying buffer to it.
    pub(crate) unsafe fn update_image_contents_from_buf(
        &mut self,
        buffer: vk::Buffer,
        image: vk::Image,
        width: u32,
        height: u32,
    ) {
        let region = &[vk::BufferImageCopy::builder()
            // 0 specifies that the pixels are tightly packed
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(
                vk::ImageSubresourceLayers::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .mip_level(0)
                    .base_array_layer(0)
                    .layer_count(1)
                    .build(),
            )
            .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
            .image_extent(vk::Extent3D {
                width: width,
                height: height,
                depth: 1,
            })
            .build()];

        self.update_image_contents_from_buf_common(buffer, image, || region);
    }

    /// Copies a list of regions from a buffer into an image.
    ///
    /// Instead of copying the entire buffer, use a thundr::Damage to
    /// populate only certain parts of the image. `damage` takes place
    /// in the image's coordinate system.
    pub(crate) unsafe fn update_image_contents_from_damaged_buf(
        &mut self,
        buffer: vk::Buffer,
        image: vk::Image,
        damage: &Damage,
    ) {
        log::debug!("Updating image with damage: {:?}", damage);
        assert!(damage.d_regions.len() > 0);

        let mut regions = Vec::new();

        for d in damage.d_regions.iter() {
            regions.push(
                vk::BufferImageCopy::builder()
                    // 0 specifies that the pixels are tightly packed
                    .buffer_offset(0)
                    .buffer_row_length(0)
                    .buffer_image_height(0)
                    .image_subresource(
                        vk::ImageSubresourceLayers::builder()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .mip_level(0)
                            .base_array_layer(0)
                            .layer_count(1)
                            .build(),
                    )
                    .image_offset(vk::Offset3D {
                        x: d.r_pos.0,
                        y: d.r_pos.1,
                        z: 0,
                    })
                    .image_extent(vk::Extent3D {
                        width: d.r_size.0 as u32,
                        height: d.r_size.1 as u32,
                        depth: 1,
                    })
                    .build(),
            );
        }

        self.update_image_contents_from_buf_common(buffer, image, || regions.as_slice());
    }

    /// This function performs common setup, completion for update functions.
    ///
    /// It handles fence waiting and cbuf recording.
    pub(crate) unsafe fn update_image_contents_from_buf_common<'a, F>(
        &mut self,
        buffer: vk::Buffer,
        image: vk::Image,
        get_regions: F,
    ) where
        F: FnOnce() -> &'a [vk::BufferImageCopy],
    {
        self.wait_for_prev_submit();
        self.wait_for_copy();
        // unsignal it, may be extraneous
        self.dev.reset_fences(&[self.copy_cbuf_fence]).unwrap();

        // now perform the copy
        self.cbuf_begin_recording(self.copy_cbuf, vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        // transition our image to be a transfer destination
        self.transition_image_layout(
            image,
            self.copy_cbuf,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        );

        let regions = get_regions();
        log::debug!("Copy image with regions: {:?}", regions);
        self.dev.cmd_copy_buffer_to_image(
            self.copy_cbuf,
            buffer,
            image,
            // this is the layout the image is currently using
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            regions,
        );

        // transition back to the optimal color layout
        self.transition_image_layout(
            image,
            self.copy_cbuf,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
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
    }

    /// Update a Vulkan image from a raw memory region
    ///
    /// This will upload the MemImage to the tansfer buffer, copy it to the image,
    /// and perform any needed layout conversions along the way
    pub unsafe fn update_image_from_memimg(&mut self, image: vk::Image, memimg: &MemImage) {
        self.wait_for_prev_submit();

        // Now copy the bits into the image
        self.upload_memimage_to_transfer(memimg);

        // Reset the fences for our cbuf submission below
        self.dev.reset_fences(&[self.copy_cbuf_fence]).unwrap();

        // transition us into the appropriate memory layout for shaders
        self.cbuf_begin_recording(self.copy_cbuf, vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

        // First thing to do here is to copy the transfer memory into the image
        let layout_barrier = vk::ImageMemoryBarrier::builder()
            .image(image)
            .src_access_mask(vk::AccessFlags::default())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
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
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[layout_barrier],
        );

        self.dev.cmd_copy_buffer_to_image(
            self.copy_cbuf,
            self.transfer_buf,
            image,
            // this is the layout the image is currently using
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            // Region to copy
            &[vk::BufferImageCopy::builder()
                // 0 specifies that the pixels are tightly packed
                .buffer_offset(0)
                // add stride
                // This will have been set in the memimg, defaulting to
                // 0 which means tightly packed.
                .buffer_row_length(memimg.stride)
                .buffer_image_height(0)
                .image_subresource(
                    vk::ImageSubresourceLayers::builder()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .mip_level(0)
                        .base_array_layer(0)
                        .layer_count(1)
                        .build(),
                )
                .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                .image_extent(vk::Extent3D {
                    width: memimg.width as u32,
                    height: memimg.height as u32,
                    depth: 1,
                })
                .build()],
        );

        // Now we need to turn this image's format back into the optimal
        // shader layout
        let dst_stage = match self.r_pipe_type {
            PipelineType::GEOMETRIC => vk::PipelineStageFlags::FRAGMENT_SHADER,
            PipelineType::COMPUTE => vk::PipelineStageFlags::COMPUTE_SHADER,
            PipelineType::ALL => {
                vk::PipelineStageFlags::FRAGMENT_SHADER | vk::PipelineStageFlags::COMPUTE_SHADER
            }
        };
        let layout_barrier = vk::ImageMemoryBarrier::builder()
            .image(image)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
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
            vk::PipelineStageFlags::TRANSFER,
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
    }

    /// Create a new image, and fill it with `data`
    ///
    /// This is meant for loading a texture into an image.
    /// It essentially just wraps `create_image` and
    /// `update_memory`.
    ///
    /// The resulting image will be in the shader read layout
    pub(crate) unsafe fn create_image_with_contents(
        &mut self,
        resolution: &vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect_flags: vk::ImageAspectFlags,
        mem_flags: vk::MemoryPropertyFlags,
        src_buf: vk::Buffer,
    ) -> (vk::Image, vk::ImageView, vk::DeviceMemory) {
        let (image, view, img_mem) = Renderer::create_image(
            &self.dev,
            &self.mem_props,
            resolution,
            format,
            usage,
            aspect_flags,
            mem_flags,
            vk::ImageTiling::OPTIMAL,
        );

        self.update_image_contents_from_buf(src_buf, image, resolution.width, resolution.height);

        (image, view, img_mem)
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
        let bindless_pool = dev.create_descriptor_pool(&info, None).unwrap();

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
            // the window order list
            vk::DescriptorSetLayoutBinding::builder()
                .binding(1)
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
                .binding(2)
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
                vk::DescriptorBindingFlags::empty(), // the window order list
                Self::get_bindless_desc_flags(),     // The unbounded array of images
            ])
            .build();
        info.p_next = &usage_info as *const _ as *mut std::ffi::c_void;

        let bindless_layout = dev.create_descriptor_set_layout(&info, None).unwrap();

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

        self.dev.destroy_buffer(self.r_windows_buf, None);
        self.free_memory(self.r_windows_mem);

        // create our data and a storage buffer for the window list
        let (wl_storage, wl_storage_mem) = self.create_buffer_with_size(
            vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::DEVICE_LOCAL
                | vk::MemoryPropertyFlags::HOST_VISIBLE
                | vk::MemoryPropertyFlags::HOST_COHERENT,
            (std::mem::size_of::<Window>() * capacity) as u64 + WINDOW_LIST_GLSL_OFFSET as u64,
        );
        self.dev
            .bind_buffer_memory(wl_storage, wl_storage_mem, 0)
            .unwrap();
        self.r_windows_buf = wl_storage;
        self.r_windows_mem = wl_storage_mem;
        self.r_windows_capacity = capacity;
    }

    /// This is a helper for reallocating the vulkan resources of the window order list
    unsafe fn reallocate_order_buf_with_cap(&mut self, capacity: usize) {
        self.wait_for_prev_submit();

        self.dev.destroy_buffer(self.r_order_buf, None);
        self.free_memory(self.r_order_mem);

        // create our data and a storage buffer for the window list
        let (wl_storage, wl_storage_mem) = self.create_buffer_with_size(
            vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::DEVICE_LOCAL
                | vk::MemoryPropertyFlags::HOST_VISIBLE
                | vk::MemoryPropertyFlags::HOST_COHERENT,
            (std::mem::size_of::<i32>() * 4 * (capacity / 4 + 1)) as u64
                + WINDOW_LIST_GLSL_OFFSET as u64,
        );
        self.dev
            .bind_buffer_memory(wl_storage, wl_storage_mem, 0)
            .unwrap();
        self.r_order_buf = wl_storage;
        self.r_order_mem = wl_storage_mem;
        self.r_order_capacity = capacity;
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
    pub fn new(info: &CreateInfo, ecs: &ECSInstance) -> Result<Renderer> {
        unsafe {
            let (entry, inst) = Renderer::create_instance(info);

            let (dr_loader, d_callback) = Renderer::setup_debug(&entry, &inst);

            let pdev = Renderer::select_pdev(&inst);

            // Our display is in charge of choosing a medium to draw on,
            // and will create a surface on that medium
            let display = Display::new(info, &entry, &inst, pdev);

            let graphics_queue_family = Renderer::select_queue_family(
                &inst,
                pdev,
                &display.d_surface_loader,
                display.d_surface,
                vk::QueueFlags::GRAPHICS,
            );
            let transfer_queue_family = Renderer::select_queue_family(
                &inst,
                pdev,
                &display.d_surface_loader,
                display.d_surface,
                vk::QueueFlags::TRANSFER,
            );
            let mem_props = Renderer::get_pdev_mem_properties(&inst, pdev);

            // TODO: allow for multiple pipes in use at once
            let pipe_type = if info.enable_traditional_composition {
                log::debug!("Using render pipeline");
                PipelineType::GEOMETRIC
            } else {
                log::debug!("Using async compute pipeline");
                PipelineType::COMPUTE
            };
            let enabled_pipelines = vec![pipe_type];

            // do this after we have gotten a valid physical device
            let surface_format = display.select_surface_format(pdev, pipe_type)?;

            let surface_caps = display
                .d_surface_loader
                .get_physical_device_surface_capabilities(pdev, display.d_surface)
                .unwrap();
            let surface_resolution = display.select_resolution(&surface_caps);
            log::profiling!("Rendering with resolution {:?}", surface_resolution);

            // create the graphics,transfer, and pipeline specific queues
            let mut families = vec![graphics_queue_family, transfer_queue_family];

            for t in enabled_pipelines.iter() {
                if let Some(family) = t.get_queue_family(&inst, &display, pdev) {
                    families.push(family);
                }
            }
            // Remove duplicate entries to keep validation from complaining
            families.dedup();

            // This *must* be done before we create our device
            #[cfg(feature = "aftermath")]
            let aftermath = Aftermath::initialize().expect("Could not enable Nvidia Aftermath SDK");

            let dev_features = VKDeviceFeatures::new(&info, &inst, pdev);
            if !dev_features.vkc_supports_desc_indexing {
                return Err(ThundrError::VK_NOT_ALL_EXTENSIONS_AVAILABLE);
            }
            let dev = Renderer::create_device(&dev_features, &inst, pdev, families.as_slice());

            // Each window is going to have a sampler descriptor for every
            // framebuffer image. Unfortunately this means the descriptor
            // count will be runtime dependent.
            // This is an allocator for those descriptors
            let descpool = DescPool::create(&dev);
            let sampler = Renderer::create_sampler(&dev);

            let present_queue = dev.get_device_queue(graphics_queue_family, 0);
            let transfer_queue = dev.get_device_queue(transfer_queue_family, 0);

            let swapchain_loader = khr::Swapchain::new(&inst, &dev);
            let swapchain = Renderer::create_swapchain(
                &inst,
                &swapchain_loader,
                &display.d_surface_loader,
                pdev,
                display.d_surface,
                &surface_caps,
                surface_format,
                &surface_resolution,
                &dev_features,
                pipe_type,
            );

            let (images, image_views) = Renderer::select_images_and_views(
                &inst,
                pdev,
                &swapchain_loader,
                swapchain,
                &dev,
                surface_format,
            );

            let pool = Renderer::create_command_pool(&dev, graphics_queue_family);
            let buffers = Renderer::create_command_buffers(&dev, pool, images.len() as u32);

            let sema_create_info = vk::SemaphoreCreateInfo::default();

            let present_sema = dev.create_semaphore(&sema_create_info, None).unwrap();
            let render_sema = dev.create_semaphore(&sema_create_info, None).unwrap();

            let fence = dev
                .create_fence(
                    &vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
                .expect("Could not create fence");

            let ext_mem_loader = khr::ExternalMemoryFd::new(&inst, &dev);

            // Create a cbuf for copying data to shm images
            let copy_cbuf = Renderer::create_command_buffers(&dev, pool, 1)[0];

            // Make a fence which will be signalled after
            // copies are completed
            let copy_fence = dev
                .create_fence(
                    &vk::FenceCreateInfo::builder().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
                .expect("Could not create fence");

            let damage_regs = std::iter::repeat(Vec::new()).take(images.len()).collect();

            // TODO:
            // We need to handle the case where the ISV doesn't support
            // a large enough number of bound samplers. In that case, I guess
            // we need to do multiple instanced draw calls of the largest
            // size supported. This will only be doable with geom I guess
            // On moltenvk this is like 128, so that's bad
            let (bindless_pool, bindless_layout) =
                Self::allocate_bindless_resources(&dev, dev_features.max_sampler_count);
            let bindless_desc =
                Self::allocate_bindless_desc(&dev, bindless_pool, &[bindless_layout], 1);

            let (tmp, tmp_view, tmp_mem) = Self::create_image(
                &dev,
                &mem_props,
                &vk::Extent2D {
                    width: 2,
                    height: 2,
                },
                surface_format.format,
                vk::ImageUsageFlags::SAMPLED,
                vk::ImageAspectFlags::COLOR,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::ImageTiling::LINEAR,
            );

            // you are now the proud owner of a half complete
            // rendering context
            // p.s. you still need a Pipeline
            let mut rend = Renderer {
                debug_loader: dr_loader,
                debug_callback: d_callback,
                loader: entry,
                inst: inst,
                dev: dev,
                dev_features: dev_features,
                pdev: pdev,
                mem_props: mem_props,
                graphics_family_index: graphics_queue_family,
                transfer_family_index: transfer_queue_family,
                present_queue: present_queue,
                transfer_queue: transfer_queue,
                display: display,
                surface_format: surface_format,
                surface_caps: surface_caps,
                resolution: surface_resolution,
                swapchain_loader: swapchain_loader,
                swapchain: swapchain,
                current_image: 0,
                fb_count: images.len(),
                swap_ages: std::iter::repeat(0).take(images.len()).collect(),
                damage_regions: damage_regs,
                current_damage: Vec::new(),
                images: images,
                image_sampler: sampler,
                views: image_views,
                pool: pool,
                cbufs: buffers,
                present_sema: present_sema,
                render_sema: render_sema,
                submit_fence: fence,
                external_mem_fd_loader: ext_mem_loader,
                r_release: Vec::new(),
                copy_cbuf: copy_cbuf,
                copy_cbuf_fence: copy_fence,
                desc_pool: descpool,
                transfer_buf: vk::Buffer::null(), // Initialize in its own method
                transfer_mem: vk::DeviceMemory::null(),
                transfer_buf_len: 0,
                draw_call_submitted: false,
                r_pipe_type: pipe_type,
                r_image_infos: Vec::new(),
                r_images_desc_pool: bindless_pool,
                r_images_desc_layout: bindless_layout,
                r_images_desc: bindless_desc,
                r_windows: ECSTable::new(ecs.clone()),
                r_windows_buf: vk::Buffer::null(),
                r_windows_mem: vk::DeviceMemory::null(),
                r_windows_capacity: 8,
                r_window_order: Vec::new(),
                r_order_buf: vk::Buffer::null(),
                r_order_mem: vk::DeviceMemory::null(),
                r_order_capacity: 8,
                tmp_image: tmp,
                tmp_image_view: tmp_view,
                tmp_image_mem: tmp_mem,
                r_acquire_barriers: Vec::new(),
                r_release_barriers: Vec::new(),
                #[cfg(feature = "aftermath")]
                r_aftermath: aftermath,
            };
            rend.reallocate_windows_buf_with_cap(rend.r_windows_capacity);
            rend.reallocate_order_buf_with_cap(rend.r_order_capacity);

            return Ok(rend);
        }
    }

    /// Recursively update the shader window parameters for surf
    ///
    /// This is used to push all CPU-side thundr data to the GPU for the shader
    /// to ork with. The offset is used through this to calculate the position of
    /// the subsurfaces relative to their parent.
    /// The flush argument forces the surface's data to be written back.
    fn update_window_list_recurse(&mut self, mut surf: Surface, offset: (i32, i32), flush: bool) {
        {
            // Only draw this surface if it has contents defined. Either
            // an image or a color
            //
            // Add this surface before its children, since we need to draw it
            // first so that any alpha in the children will see this underneath
            let internal = surf.s_internal.borrow();
            if internal.s_image.is_some() || internal.s_color.is_some() {
                self.r_window_order.push(surf.s_window_id.clone());
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
    fn update_window_list(&mut self, surfaces: &SurfaceList) {
        self.r_window_order.clear();

        for surf in surfaces.iter().rev() {
            self.update_window_list_recurse(surf.clone(), (0, 0), false);
        }
    }

    /// Write our Thundr Surface's data to the window list we will pass to the shader
    ///
    /// The shader needs a contiguous list of surfaces, so we turn our surfaces
    /// into a bunch of "windows". These windows will have their size and offset
    /// populated, along with any other drawing data. These live in r_windows, and
    /// the order is set by r_window_order.
    ///
    /// The offset parameter comes from the offset of this window due to its
    /// surface being a subsurface.
    fn update_surf_shader_window(&mut self, surf_rc: &Surface, offset: (i32, i32)) {
        // Our iterator is going to take into account the dimensions of the
        // parent surface(s), and give us the offset from which we should start
        // doing our calculations. Basically off_x is the parent surfaces X position.
        let surf = surf_rc.s_internal.borrow();
        let opaque_reg = match surf_rc.get_opaque() {
            Some(r) => r,
            // If no opaque data was attached, place a -1 in the start.x component
            // to tell the shader to ignore this
            None => Rect::new(-1, 0, -1, 0),
        };
        let (image_id, use_color) = match surf.s_image.as_ref() {
            Some(i) => (i.get_id(), false),
            None => (-1, true),
        };

        self.r_windows[&surf_rc.s_window_id] = Window {
            w_id: (image_id, use_color as i32, 0, 0),
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
        };
    }

    fn initialize_transfer_mem(&mut self) {
        let transfer_buf_len = 64;
        let (buffer, buf_mem) = unsafe {
            self.create_buffer_with_size(
                vk::BufferUsageFlags::TRANSFER_SRC,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                transfer_buf_len,
            )
        };

        self.transfer_buf_len = transfer_buf_len as usize;
        self.transfer_buf = buffer;
        self.transfer_mem = buf_mem;
    }

    pub fn upload_memimage_to_transfer(&mut self, memimg: &MemImage) {
        unsafe {
            // We might be in the middle of copying the transfer buf to an image
            // wait for that if its the case
            self.wait_for_copy();
            if memimg.as_slice().len() > self.transfer_buf_len {
                // WHY DOES COMMENTING THESE OUT FIX THINGS??
                self.free_memory(self.transfer_mem);
                self.dev.destroy_buffer(self.transfer_buf, None);
                let (buffer, buf_mem) = self.create_buffer(
                    vk::BufferUsageFlags::TRANSFER_SRC,
                    vk::SharingMode::EXCLUSIVE,
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                    memimg.as_slice(),
                );
                self.transfer_buf = buffer;
                self.transfer_mem = buf_mem;
                self.transfer_buf_len = memimg.as_slice().len();
            } else {
                // copy the data into the staging buffer
                self.update_memory(self.transfer_mem, 0, memimg.as_slice());
            }
        }
    }

    /// Wait for the submit_fence
    pub fn wait_for_prev_submit(&self) {
        unsafe {
            match self.dev.wait_for_fences(
                &[self.submit_fence, self.copy_cbuf_fence],
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
                            self.r_aftermath.wait_for_dump();
                        }
                    }
                    _ => panic!("Could not wait for vulkan fences"),
                },
            };
        }
    }

    pub fn wait_for_copy(&self) {
        unsafe {
            self.dev
                .wait_for_fences(
                    &[self.copy_cbuf_fence],
                    true,          // wait for all
                    std::u64::MAX, //timeout
                )
                .expect("Could not wait for the copy fence");
        }
    }

    /// Records and submits a one-time command buffer.
    ///
    /// cbuf - the command buffer to use
    /// queue - the queue to submit cbuf to
    /// wait_stages - a list of pipeline stages to wait on
    /// wait_semas - semaphores we consume
    /// signal_semas - semaphores we notify
    ///
    /// All operations in the `record_fn` argument will be
    /// submitted in the command buffer `cbuf`. This aims to make
    /// constructing buffers more ergonomic.
    pub(crate) fn cbuf_submit_and_wait(
        &self,
        cbuf: vk::CommandBuffer,
        queue: vk::Queue,
        wait_stages: &[vk::PipelineStageFlags],
        wait_semas: &[vk::Semaphore],
        signal_semas: &[vk::Semaphore],
    ) {
        self.cbuf_end_recording(cbuf);

        unsafe {
            // once the one-time buffer has been recorded we can submit
            // it for execution.
            // Interesting: putting the cbuf into a list in the builder
            // struct makes it segfault in release mode... Deep dive
            // needed...
            let cbufs = [cbuf];
            let submit_info = vk::SubmitInfo::builder()
                .wait_semaphores(wait_semas)
                .wait_dst_stage_mask(wait_stages)
                .command_buffers(&cbufs)
                .signal_semaphores(signal_semas)
                .build();

            let fence = self
                .dev
                .create_fence(&vk::FenceCreateInfo::default(), None)
                .expect("Could not create fence");

            // create a fence to be notified when the commands have finished
            // executing. Wait immediately for the fence.
            self.dev
                .queue_submit(queue, &[submit_info], fence)
                .expect("Could not submit buffer to queue");

            self.dev
                .wait_for_fences(
                    &[fence],
                    true,          // wait for all
                    std::u64::MAX, //timeout
                )
                .expect("Could not wait for the submit fence");
            // the commands are now executed
            self.dev.destroy_fence(fence, None);
        }
    }

    /// Submits a command buffer.
    ///
    /// This is used for synchronized submits for graphical
    /// display operations. It waits for submit_fence before
    /// submitting to queue, and will signal it when the
    /// cbuf is executed. (see cbuf_sumbmit_async)
    ///
    /// The buffer MUST have been recorded before this
    ///
    /// cbuf - the command buffer to use
    /// queue - the queue to submit cbuf to
    /// wait_stages - a list of pipeline stages to wait on
    /// wait_semas - semaphores we consume
    /// signal_semas - semaphores we notify
    pub(crate) fn cbuf_submit(
        &self,
        cbuf: vk::CommandBuffer,
        queue: vk::Queue,
        wait_stages: &[vk::PipelineStageFlags],
        wait_semas: &[vk::Semaphore],
        signal_semas: &[vk::Semaphore],
    ) {
        unsafe {
            self.wait_for_prev_submit();
            self.dev.reset_fences(&[self.submit_fence]).unwrap();

            self.cbuf_submit_async(
                cbuf,
                queue,
                // wait_stages
                wait_stages,
                wait_semas,
                signal_semas,
                self.submit_fence,
            );
        }
    }

    /// Submits a command buffer asynchronously.
    ///
    /// Simple wrapper for queue submission. Does not
    /// wait for anything.
    ///
    /// The buffer MUST have been recorded before this
    ///
    /// cbuf - the command buffer to use
    /// queue - the queue to submit cbuf to
    /// wait_stages - a list of pipeline stages to wait on
    /// wait_semas - semaphores we consume
    /// signal_semas - semaphores we notify
    pub(crate) fn cbuf_submit_async(
        &self,
        cbuf: vk::CommandBuffer,
        queue: vk::Queue,
        wait_stages: &[vk::PipelineStageFlags],
        wait_semas: &[vk::Semaphore],
        signal_semas: &[vk::Semaphore],
        signal_fence: vk::Fence,
    ) {
        unsafe {
            // The buffer must have been recorded before we can submit
            // it for execution.
            let submit_info = vk::SubmitInfo::builder()
                .wait_semaphores(wait_semas)
                .wait_dst_stage_mask(wait_stages)
                .command_buffers(&[cbuf])
                .signal_semaphores(signal_semas)
                .build();

            // create a fence to be notified when the commands have finished
            // executing.
            self.dev
                .queue_submit(queue, &[submit_info], signal_fence)
                .unwrap();
        }
    }

    /// Records but does not submit a command buffer.
    ///
    /// cbuf - the command buffer to use
    /// flags - the usage flags for the buffer
    ///
    /// All operations in the `record_fn` argument will be
    /// recorded in the command buffer `cbuf`.
    pub fn cbuf_begin_recording(
        &self,
        cbuf: vk::CommandBuffer,
        flags: vk::CommandBufferUsageFlags,
    ) {
        unsafe {
            // first reset the queue so we know it is empty
            self.dev
                .reset_command_buffer(cbuf, vk::CommandBufferResetFlags::RELEASE_RESOURCES)
                .expect("Could not reset command buffer");

            // this cbuf will only be used once, so tell vulkan that
            // so it can optimize accordingly
            let record_info = vk::CommandBufferBeginInfo::builder().flags(flags);

            // start recording the command buffer, call the function
            // passed to load it with operations, and then end the
            // command buffer
            self.dev
                .begin_command_buffer(cbuf, &record_info)
                .expect("Could not start command buffer");
        }
    }

    /// Records but does not submit a command buffer.
    ///
    /// cbuf - the command buffer to use
    pub fn cbuf_end_recording(&self, cbuf: vk::CommandBuffer) {
        unsafe {
            self.dev
                .end_command_buffer(cbuf)
                .expect("Could not end command buffer");
        }
    }

    pub fn get_recording_parameters(&self) -> RecordParams {
        RecordParams {
            cbuf: self.cbufs[self.current_image as usize],
            image_num: self.current_image as usize,
        }
    }

    /// Adds damage to `regions` without modifying the damage
    fn aggregate_damage(&self, damage: &Damage, regions: &mut Vec<vk::RectLayerKHR>) {
        let swapchain_extent = Rect::new(
            0,
            0,
            self.resolution.width as i32,
            self.resolution.height as i32,
        );

        for d in damage.regions() {
            // Limit the damage to the screen dimensions
            let region = d.clip(&swapchain_extent);

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

            regions.push(rect);
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
    pub fn begin_recording_one_frame(
        &mut self,
        surfaces: &mut SurfaceList,
    ) -> Result<RecordParams> {
        // At least wait for any image copies to complete
        self.wait_for_copy();
        // get the next frame to draw into
        self.get_next_swapchain_image()?;

        // TODO: redo the way I track swap ages. The order the images are acquired
        // isn't guaranteed to be constant

        // Now combine the first n lists (depending on the current
        // image's age) into one list for vkPresentRegionsKHR (and `gen_tile_list`)
        // We need to do this first since popping an entry off damage_regions
        // would remove one of the regions we need to process.
        // Using in lets us never go past the end of the array
        if self.dev_features.vkc_supports_incremental_present {
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
            let mut regions = Vec::new();

            for surf_rc in surfaces.iter_mut() {
                // add the new damage to the list of damages
                // If the surface does not have damage attached, then don't generate tiles
                if let Some(damage) = surf_rc.get_global_damage() {
                    self.aggregate_damage(&damage, &mut regions);
                }

                // now we have to consider damage caused by moving the surface
                //
                // We don't have to correct the position based on the surface pos
                // since the damage was already recorded for the surface
                if let Some(damage) = surf_rc.take_surface_damage() {
                    self.aggregate_damage(&damage, &mut regions);
                }
            }

            // Finally we add any damage that the surfacelist has
            for damage in surfaces.damage() {
                self.aggregate_damage(damage, &mut regions);
            }
            surfaces.clear_damage();

            self.current_damage.extend(&regions);
            self.damage_regions.push_front(regions);

            // Only update the ages after we have processed them
            self.update_buffer_ages();
        }

        Ok(self.get_recording_parameters())
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

        self.dev.allocate_descriptor_sets(&info).unwrap()
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

        self.dev.update_descriptor_sets(
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
        let sampler = Renderer::create_sampler(&self.dev);
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

    /// Allocates a buffer/memory pair of size `size`.
    ///
    /// This is just a helper for `create_buffer`. It does not fill
    /// the buffer with anything.
    pub unsafe fn create_buffer_with_size(
        &self,
        usage: vk::BufferUsageFlags,
        mode: vk::SharingMode,
        flags: vk::MemoryPropertyFlags,
        size: u64,
    ) -> (vk::Buffer, vk::DeviceMemory) {
        let create_info = vk::BufferCreateInfo::builder()
            .size(size)
            .usage(usage)
            .sharing_mode(mode);

        let buffer = self.dev.create_buffer(&create_info, None).unwrap();
        let req = self.dev.get_buffer_memory_requirements(buffer);
        // get the memory types for this pdev
        let props = Renderer::get_pdev_mem_properties(&self.inst, self.pdev);
        // find the memory type that best suits our requirements
        let index = Renderer::find_memory_type_index(&props, &req, flags).unwrap();

        // now we need to allocate memory to back the buffer
        let alloc_info = vk::MemoryAllocateInfo {
            allocation_size: req.size,
            memory_type_index: index,
            ..Default::default()
        };

        let memory = self.dev.allocate_memory(&alloc_info, None).unwrap();

        return (buffer, memory);
    }

    /// Wrapper for freeing device memory
    ///
    /// Having this in one place lets us quickly handle any additional
    /// allocation tracking
    pub(crate) unsafe fn free_memory(&self, mem: vk::DeviceMemory) {
        self.dev.free_memory(mem, None);
    }

    /// Writes `data` to `memory`
    ///
    /// This is a helper method for mapping and updating the value stored
    /// in device memory Memory needs to be host visible and coherent.
    /// This does not flush after writing.
    pub(crate) unsafe fn update_memory<T: Copy>(
        &self,
        memory: vk::DeviceMemory,
        offset: isize,
        data: &[T],
    ) {
        if data.len() == 0 {
            return;
        }

        // Now we copy our data into the buffer
        let data_size = std::mem::size_of_val(data) as u64;
        let ptr = self
            .dev
            .map_memory(
                memory,
                offset as u64, // offset
                data_size,
                vk::MemoryMapFlags::empty(),
            )
            .unwrap();

        // rust doesn't have a raw memcpy, so we need to transform the void
        // ptr to a slice. This is unsafe as the length needs to be correct
        let dst = std::slice::from_raw_parts_mut(ptr as *mut T, data.len());
        dst.copy_from_slice(data);

        self.dev.unmap_memory(memory);
    }

    /// allocates a buffer/memory pair and fills it with `data`
    ///
    /// There are two components to a memory backed resource in vulkan:
    /// vkBuffer which is the actual buffer itself, and vkDeviceMemory which
    /// represents a region of allocated memory to hold the buffer contents.
    ///
    /// Both are returned, as both need to be destroyed when they are done.
    pub(crate) unsafe fn create_buffer<T: Copy>(
        &self,
        usage: vk::BufferUsageFlags,
        mode: vk::SharingMode,
        flags: vk::MemoryPropertyFlags,
        data: &[T],
    ) -> (vk::Buffer, vk::DeviceMemory) {
        let size = std::mem::size_of_val(data) as u64;
        let (buffer, memory) = self.create_buffer_with_size(usage, mode, flags, size);

        self.update_memory(memory, 0, data);

        // Until now the buffer has not had any memory assigned
        self.dev.bind_buffer_memory(buffer, memory, 0).unwrap();

        (buffer, memory)
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

        unsafe { dev.allocate_descriptor_sets(&info).unwrap()[0] }
    }

    pub fn refresh_window_resources(&mut self, images: &[Image], surfaces: &mut SurfaceList) {
        self.wait_for_prev_submit();

        // Construct a list of image views from the submitted surface list
        // this will be our unsized texture array that the composite shader will reference
        // TODO: make this a changed flag
        if self.r_image_infos.len() != images.len() && images.len() > 0 {
            // free the previous descriptor sets
            unsafe {
                self.dev
                    .reset_descriptor_pool(
                        self.r_images_desc_pool,
                        vk::DescriptorPoolResetFlags::empty(),
                    )
                    .unwrap();
            }

            self.r_images_desc = Self::allocate_bindless_desc(
                &self.dev,
                self.r_images_desc_pool,
                &[self.r_images_desc_layout],
                images.len() as u32,
            );
        }

        // Now that we have possibly reallocated the descriptor sets,
        // refresh the window list to put it back in gpu mem
        self.refresh_window_list(surfaces);

        // Construct a list of image views from the submitted surface list
        // this will be our unsized texture array that the composite shader will reference
        self.r_image_infos.clear();
        for image in images.iter() {
            self.r_image_infos.push(
                vk::DescriptorImageInfo::builder()
                    .sampler(self.image_sampler)
                    // The image view could have been recreated and this would be stale
                    .image_view(image.get_view())
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .build(),
            );
        }

        if self.r_image_infos.len() == 0 {
            self.r_image_infos.push(
                vk::DescriptorImageInfo::builder()
                    .sampler(self.image_sampler)
                    .image_view(self.tmp_image_view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .build(),
            );
        }

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
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&[vk::DescriptorBufferInfo::builder()
                    .buffer(self.r_order_buf)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
                    .build()])
                .build(),
            vk::WriteDescriptorSet::builder()
                .dst_set(self.r_images_desc)
                .dst_binding(2)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(self.r_image_infos.as_slice())
                .build(),
        ];

        unsafe {
            self.dev.update_descriptor_sets(
                write_infos, // descriptor writes
                &[],         // descriptor copies
            );
        }
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

        self.ensure_window_capacity(self.r_windows.ect_data.len());

        log::info!("Window List: {:#?}", self.r_windows);

        // TODO: don't even use CPU copies of the datastructs and perform
        // the tile/window updates in the mapped GPU memory
        // (requires benchmark)
        unsafe {
            // Shader expects struct WindowList { int count; Window windows[] }
            self.update_memory(self.r_windows_mem, 0, &[self.r_windows.ect_data.len()]);
            self.update_memory(
                self.r_windows_mem,
                WINDOW_LIST_GLSL_OFFSET,
                self.r_windows.ect_data.as_slice(),
            );

            // Turn our vec of ECSIds into a vec of actual ids.
            let mut window_order = Vec::new();
            for ecs in self.r_window_order.iter() {
                window_order.push(ecs.get_raw_id() as i32);
            }
            log::debug!("Window order is {:?}", window_order);

            self.reallocate_order_buf_with_cap(self.r_window_order.len());
            self.update_memory(self.r_order_mem, 0, &[self.r_window_order.len()]);
            self.update_memory(
                self.r_order_mem,
                WINDOW_LIST_GLSL_OFFSET,
                window_order.as_slice(),
            );

            self.dev.device_wait_idle().unwrap();
        }
    }

    /// Update self.current_image with the swapchain image to render to
    ///
    /// Returns if the next image index was successfully obtained
    /// false means try again later, the next image is not ready
    pub fn get_next_swapchain_image(&mut self) -> Result<()> {
        unsafe {
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
                    Ok(())
                }
                Err(vk::Result::NOT_READY) => {
                    log::debug!(
                        "vkAcquireNextImageKHR: vk::Result::NOT_READY: Current {:?}",
                        self.current_image
                    );
                    Err(ThundrError::NOT_READY)
                }
                Err(vk::Result::TIMEOUT) => {
                    log::debug!(
                        "vkAcquireNextImageKHR: vk::Result::NOT_READY: Current {:?}",
                        self.current_image
                    );
                    Err(ThundrError::TIMEOUT)
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(ThundrError::OUT_OF_DATE),
                Err(vk::Result::SUBOPTIMAL_KHR) => Err(ThundrError::OUT_OF_DATE),
                // the call did not succeed
                Err(_) => Err(ThundrError::COULD_NOT_ACQUIRE_NEXT_IMAGE),
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
        match unsafe { self.dev.get_fence_status(self.submit_fence) } {
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

        if self.dev_features.vkc_supports_incremental_present {
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

        unsafe {
            match self
                .swapchain_loader
                .queue_present(self.present_queue, &info)
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
            self.dev.device_wait_idle().unwrap();

            self.dev.destroy_buffer(self.transfer_buf, None);
            self.dev.free_memory(self.transfer_mem, None);

            self.dev.destroy_image(self.tmp_image, None);
            self.dev.destroy_image_view(self.tmp_image_view, None);
            self.free_memory(self.tmp_image_mem);

            self.dev.destroy_semaphore(self.present_sema, None);
            self.dev.destroy_semaphore(self.render_sema, None);
            self.desc_pool.destroy(&self.dev);
            self.dev.destroy_sampler(self.image_sampler, None);
            self.dev
                .destroy_descriptor_set_layout(self.r_images_desc_layout, None);

            self.dev
                .destroy_descriptor_pool(self.r_images_desc_pool, None);
            self.dev.destroy_buffer(self.r_windows_buf, None);
            self.free_memory(self.r_windows_mem);
            self.dev.destroy_buffer(self.r_order_buf, None);
            self.free_memory(self.r_order_mem);

            self.destroy_swapchain();

            self.dev.destroy_command_pool(self.pool, None);
            self.dev.destroy_fence(self.submit_fence, None);
            self.dev.destroy_fence(self.copy_cbuf_fence, None);
            self.dev.destroy_device(None);

            self.display.destroy();

            self.debug_loader
                .destroy_debug_utils_messenger(self.debug_callback, None);
            self.inst.destroy_instance(None);
        }
    }
}
