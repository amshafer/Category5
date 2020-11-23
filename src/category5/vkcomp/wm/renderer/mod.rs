// A vulkan rendering backend
//
// This layer is very low, and as a result is mostly unsafe. Nothing
// unsafe/vulkan/ash/etc should be exposed to upper layers
//
// Austin Shafer - 2020
#![allow(dead_code, non_camel_case_types)]
use serde::{Serialize, Deserialize};

use cgmath::{Vector3,Vector2,Matrix4};

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::io::Cursor;
use std::marker::Copy;
use std::mem;
use std::cell::RefCell;

use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0};
use ash::{vk, Device, Entry, Instance};
use ash::util;
use ash::extensions::ext::DebugReport;
use ash::extensions::khr;

mod descpool;
use descpool::DescPool;

mod display;
use display::Display;

pub mod mesh;
use mesh::Mesh;

use super::App;
use crate::log;
use crate::category5::utils::{*, timing::*, logging::LogLevel};

// This is the reference data for a normal quad
// that will be used to draw client windows.
static QUAD_DATA: [VertData; 4] = [
    VertData {
        vertex: Vector2::new(0.0, 0.0),
        tex: Vector2::new(0.0, 0.0),
    },
    VertData {
        vertex: Vector2::new(1.0, 0.0),
        tex: Vector2::new(1.0, 0.0),
    },
    VertData {
        vertex: Vector2::new(0.0, 1.0),
        tex: Vector2::new(0.0, 1.0),
    },
    VertData {
        vertex: Vector2::new(1.0, 1.0),
        tex: Vector2::new(1.0, 1.0),
    },
];

static QUAD_INDICES: [Vector3::<u32>; 2] = [
    Vector3::new(1, 2, 3),
    Vector3::new(1, 4, 2),
];

// this happy little debug callback is from the ash examples
// all it does is print any errors/warnings thrown.
unsafe extern "system" fn vulkan_debug_callback(
    _: vk::DebugReportFlagsEXT,
    _: vk::DebugReportObjectTypeEXT,
    _: u64,
    _: usize,
    _: i32,
    _: *const c_char,
    p_message: *const c_char,
    _: *mut c_void,
) -> u32 {
    log!(LogLevel::profiling, "[RENDERER] {:?}", CStr::from_ptr(p_message));
    vk::FALSE
}

// Behold a vulkan rendering context
//
// The fields here are sure to change, as they are pretty
// application specific.
//
// The types in ash::vk:: are the 'normal' vulkan types
// types in ash:: are normally 'loaders'. They take care of loading
// function pointers and things. Think of them like a wrapper for
// the raw vk:: type. In some cases you need both, surface
// is a good example of this.
//
// Application specific fields should be at the bottom of the
// struct, with the commonly required fields at the top.
pub struct Renderer {
    // debug callback sugar mentioned earlier
    debug_loader: DebugReport,
    debug_callback: vk::DebugReportCallbackEXT,

    // the entry just loads function pointers from the dynamic library
    // I am calling it a loader, because that's what it does
    loader: Entry,
    // the big vulkan instance.
    inst: Instance,
    // the logical device we are using
    // maybe I'll test around with multi-gpu
    dev: Device,
    // the physical device selected to display to
    pdev: vk::PhysicalDevice,
    mem_props: vk::PhysicalDeviceMemoryProperties,

    // index into the array of queue families
    graphics_family_index: u32,
    transfer_family_index: u32,
    // processes things to be physically displayed
    present_queue: vk::Queue,
    // queue for copy operations
    transfer_queue: vk::Queue,

    // vk_khr_display and vk_khr_surface wrapper.
    display: Display,
    surface_format: vk::SurfaceFormatKHR,
    surface_caps: vk::SurfaceCapabilitiesKHR,
    // resolution to create the swapchain with
    pub resolution: vk::Extent2D,

    // loads swapchain extension
    swapchain_loader: khr::Swapchain,
    // the actual swapchain
    swapchain: vk::SwapchainKHR,
    // index into swapchain images that we are currently using
    current_image: u32,

    // a set of images belonging to swapchain
    images: Vec<vk::Image>,
    // number of framebuffers (2 is double buffering)
    pub fb_count: usize,
    // views describing how to access the images
    views: Vec<vk::ImageView>,

    // pools provide the memory allocated to command buffers
    pool: vk::CommandPool,
    // the command buffers allocated from pool
    cbufs: Vec<vk::CommandBuffer>,

    // ---- Application specific ----
    app_ctx: RefCell<Option<AppContext>>,

    // an image for recording depth test data
    depth_image: vk::Image,
    depth_image_view: vk::ImageView,
    // because we create the image, we need to back it with memory
    depth_image_mem: vk::DeviceMemory,

    // This signals that the latest contents have been presented.
    // It is signaled by acquire next image and is consumed by
    // the cbuf submission
    present_sema: vk::Semaphore,
    // This is signaled by start_frame, and is consumed by present.
    // This keeps presentation from occurring until rendering is
    // complete
    render_sema: vk::Semaphore,
    // This fence coordinates draw call reuse. It will be signaled
    // when submitting the draw calls to the queue has finished
    submit_fence: vk::Fence,
    // needed for VkGetMemoryFdPropertiesKHR
    external_mem_fd_loader: khr::ExternalMemoryFd,
    // The pending release list
    // This is the set of wayland resources used last frame
    // for rendering that should now be released
    // See WindowManger's worker_thread for more
    r_release: Vec<ReleaseInfo>,
}

// an application specific set of resources to draw.
//
// These are the "dynamic" parts of our application. The things
// that change depending on the scene. It holds pipelines, layouts
// shaders, and geometry.
//
// Ideally the `Renderer` can render/present anything, and this
// struct specifies what to draw. This allows the second half
// of the initialization functions to just have a self ref.
//
// meshes are created with Renderer::create_mesh. The renderer is in
// charge of creating/destroying the meshes since all of the mesh
// resources are created from the Renderer.
pub struct AppContext {
    pass: vk::RenderPass,
    pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    // This descriptor pool allocates only the 1 ubo
    uniform_pool: vk::DescriptorPool,
    // This is an allocator for the dynamic sets (samplers)
    desc_pool: DescPool,
    // (as per `create_descriptor_layouts`)
    // This will only be the sets holding the uniform buffers,
    // any mesh specific descriptors are in the mesh's sets.
    descriptor_uniform_layout: vk::DescriptorSetLayout,
    ubo_descriptor: vk::DescriptorSet,
    shader_modules: Vec<vk::ShaderModule>,
    framebuffers: Vec<vk::Framebuffer>,
    // shader constants are shared by all swapchain images
    uniform_buffer: vk::Buffer,
    uniform_buffers_memory: vk::DeviceMemory,
    // TODO: this should probably be a uniform texel buffer
    // One sampler for each swapchain image
    image_sampler: vk::Sampler,
    // We will hold only one copy of the static QUAD_DATA
    // which represents an onscreen window.
    vert_buffer: vk::Buffer,
    vert_buffer_memory: vk::DeviceMemory,
    vert_count: u32,
    // Resources for the index buffer
    index_buffer: vk::Buffer,
    index_buffer_memory: vk::DeviceMemory,
    // command buffer for copying shm images
    copy_cbuf: vk::CommandBuffer,
    copy_cbuf_fence: vk::Fence,
}

// Recording parameters
//
// Layers above this one will need to call recording
// operations. They need a private structure to pass
// to Renderer to begin/end recording operations
// This is that structure.
pub struct RecordParams {
    cbuf: vk::CommandBuffer,
    image_num: usize,
}

// Contiains a vertex and all its related data
//
// Things like vertex normals and colors will be passed in
// the same vertex input assembly, so this type provides
// a wrapper for handling all of them at once.
#[repr(C)]
#[derive(Clone,Copy)]
struct VertData {
    vertex: Vector2<f32>,
    tex: Vector2<f32>,
}

// Shader constants are used for
// the larger uniform values which are
// not changed very often.
#[derive(Clone,Copy)]
#[repr(C)]
struct ShaderConstants {
    model: Matrix4<f32>,
    // Maybe these should be floats for HiDPI?
    width: f32,
    height: f32,
}

// Push constants are used for small bits of data
// which are changed often. We will use them to
// transform the default square into the size of
// the client window.
//
// This should to be less than 128 bytes to guarantee
// that there will be enough push constant space.
#[derive(Clone,Copy,Serialize,Deserialize)]
#[repr(C)]
pub struct PushConstants {
    // the z-ordering of the window being drawn
    pub order: f32,
    // this is [0,resolution]
    pub x: f32,
    pub y: f32,
    // Maybe these should be floats for HiDPI?
    pub width: f32,
    pub height: f32,
}

// Most of the functions below will be unsafe. Only the safe functions
// should be used by the applications. The unsafe functions are mostly for
// internal use.
impl Renderer {

    // Creates a new debug reporter and registers our function
    // for debug callbacks so we get nice error messages
    unsafe fn setup_debug(entry: &Entry, instance: &Instance)
                          -> (DebugReport, vk::DebugReportCallbackEXT)
    {
        let debug_info = vk::DebugReportCallbackCreateInfoEXT::builder()
            .flags(
                vk::DebugReportFlagsEXT::ERROR
                    | vk::DebugReportFlagsEXT::WARNING
                    | vk::DebugReportFlagsEXT::PERFORMANCE_WARNING,
            )
            .pfn_callback(Some(vulkan_debug_callback));

        let dr_loader = DebugReport::new(entry, instance);
        let callback = dr_loader
            .create_debug_report_callback(&debug_info, None)
            .unwrap();
        return (dr_loader, callback);
    }

    // Create a vkInstance
    //
    // Most of the create info entries are straightforward, with
    // some basic extensions being enabled. All of the work is
    // done in subfunctions.
    unsafe fn create_instance() -> (Entry, Instance) {
        let entry = Entry::new().unwrap();
        let app_name = CString::new("VulkanRenderer").unwrap();

        let layer_names = [];

        let layer_names_raw: Vec<*const i8> = layer_names.iter()
            .map(|raw_name: &CString| raw_name.as_ptr())
            .collect();

        let extension_names_raw = Display::extension_names();

        let appinfo = vk::ApplicationInfo::builder()
            .application_name(&app_name)
            .application_version(0)
            .engine_name(&app_name)
            .engine_version(0)
            .api_version(vk::make_version(1, 1, 127));

        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(&appinfo)
            .enabled_layer_names(&layer_names_raw)
            .enabled_extension_names(&extension_names_raw);

        let instance: Instance = entry
            .create_instance(&create_info, None)
            .expect("Instance creation error");

        return (entry, instance);
    }

    // Check if a queue family is suited for our needs.
    // Queue families need to support graphical presentation and 
    // presentation on the given surface.
    unsafe fn is_valid_queue_family(pdevice: vk::PhysicalDevice,
                                        info: vk::QueueFamilyProperties,
                                        index: u32,
                                        surface_loader: &khr::Surface,
                                        surface: vk::SurfaceKHR,
                                        flags: vk::QueueFlags)
                                        -> bool
    {
        info.queue_flags.contains(flags)
            && surface_loader
            // ensure compatibility with the surface
            .get_physical_device_surface_support(
                pdevice,
                index,
                surface,
            ).unwrap()
    }

    // Choose a vkPhysicalDevice and queue family index
    //
    // selects a physical device and a queue family
    // provide the surface PFN loader and the surface so
    // that we can ensure the pdev/queue combination can
    // present the surface
    unsafe fn select_pdev(inst: &Instance)
                              -> vk::PhysicalDevice
    {
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

    // Choose a queue family
    //
    // returns an index into the array of queue types.
    // provide the surface PFN loader and the surface so
    // that we can ensure the pdev/queue combination can
    // present the surface
    unsafe fn select_queue_family(inst: &Instance,
                                      pdev: vk::PhysicalDevice,
                                      surface_loader: &khr::Surface,
                                      surface: vk::SurfaceKHR,
                                      flags: vk::QueueFlags)
                                      -> u32
    {
        // get the properties per queue family
        inst
            .get_physical_device_queue_family_properties(pdev)
            // for each property info
            .iter()
            .enumerate()
            .filter_map(|(index, info)| {
                // add the device and the family to a list of
                // candidates for use later
                match Renderer::is_valid_queue_family(pdev,
                                                      *info,
                                                      index as u32,
                                                      surface_loader,
                                                      surface,
                                                      flags) {
                    // return the pdevice/family pair
                    true => Some(index as u32),
                    false => None,
                }
            })
            .nth(0)
            .expect("Could not find a suitable queue family")
    }

    // get the vkPhysicalDeviceMemoryProperties structure for a vkPhysicalDevice
    unsafe fn get_pdev_mem_properties(inst: &Instance,
                                          pdev: vk::PhysicalDevice)
                                          -> vk::PhysicalDeviceMemoryProperties
    {
        inst.get_physical_device_memory_properties(pdev)
    }

    // Create a vkDevice from a vkPhysicalDevice
    //
    // Create a logical device for interfacing with the physical device.
    // once again we specify any device extensions we need, the swapchain
    // being the most important one.
    //
    // A queue is created in the specified queue family in the
    // present_queue argument.
    unsafe fn create_device(inst: &Instance,
                                pdev: vk::PhysicalDevice,
                                queues: &[u32])
                                -> Device
    {
        let dev_extension_names = [khr::Swapchain::name().as_ptr(),
                                   khr::ExternalMemoryFd::name().as_ptr()];

        let features = vk::PhysicalDeviceFeatures {
            shader_clip_distance: 1,
            ..Default::default()
        };

        // for now we only have one graphics queue, so one priority
        let priorities = [1.0];
        let mut queue_infos = Vec::new();
        for i in queues {
            queue_infos.push(vk::DeviceQueueCreateInfo::builder()
                             .queue_family_index(*i)
                             .queue_priorities(&priorities)
                             .build());
        }

        let dev_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(queue_infos.as_ref())
            .enabled_extension_names(&dev_extension_names)
            .enabled_features(&features)
            .build();

        // return a newly created device
        inst.create_device(pdev, &dev_create_info, None)
            .unwrap()
    }

    // create a new vkSwapchain
    //
    // Swapchains contain images that can be used for WSI presentation
    // They take a vkSurfaceKHR and provide a way to manage swapping
    // effects such as double/triple buffering (mailbox mode). The created
    // swapchain is dependent on the characteristics and format of the surface
    // it is created for.
    // The application resolution is set by this method.
    unsafe fn create_swapchain(swapchain_loader: &khr::Swapchain,
                                   surface_loader: &khr::Surface,
                                   pdev: vk::PhysicalDevice,
                                   surface: vk::SurfaceKHR,
                                   surface_caps: &vk::SurfaceCapabilitiesKHR,
                                   surface_format: vk::SurfaceFormatKHR,
                                   resolution: &vk::Extent2D)
                                   -> vk::SwapchainKHR
    {
        // how many images we want the swapchain to contain
        let mut desired_image_count = surface_caps.min_image_count + 1;
        if surface_caps.max_image_count > 0
            && desired_image_count > surface_caps.max_image_count
        {
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

        let create_info = vk::SwapchainCreateInfoKHR::builder()
            .surface(surface)
            .min_image_count(desired_image_count)
            .image_color_space(surface_format.color_space)
            .image_format(surface_format.format)
            .image_extent(*resolution)
            // the color attachment is guaranteed to be available
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(mode)
            .clipped(true)
            .image_array_layers(1);

        // views for all of the swapchains images will be set up in
        // select_images_and_views
        swapchain_loader
            .create_swapchain(&create_info, None)
            .unwrap()
    }

    // returns a new vkCommandPool
    //
    // Command buffers are allocated from command pools. That's about
    // all they do. They just manage memory. Command buffers will be allocated
    // as part of the queue_family specified.
    unsafe fn create_command_pool(dev: &Device,
                                  queue_family: u32)
                                  -> vk::CommandPool
    {
        let pool_create_info = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);

        dev.create_command_pool(&pool_create_info, None).unwrap()
    }

    // Allocate a vec of vkCommandBuffers
    //
    // Command buffers are constructed once, and can be executed
    // many times. They also have the added bonus of being added to
    // by multiple threads. Command buffer is shortened to `cbuf` in
    // many areas of the code.
    //
    // For now we are only allocating two: one to set up the resources
    // and one to do all the work.
    unsafe fn create_command_buffers(dev: &Device,
                                     pool: vk::CommandPool,
                                     count: u32)
                                     -> Vec<vk::CommandBuffer>
    {
        let cbuf_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(count)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY);

        dev.allocate_command_buffers(&cbuf_allocate_info)
            .unwrap()
    }

    // Get the vkImage's for the swapchain, and create vkImageViews for them
    //
    // get all the presentation images for the swapchain
    // specify the image views, which specify how we want
    // to access our images
    unsafe fn select_images_and_views(swapchain_loader: &khr::Swapchain,
                                      swapchain: vk::SwapchainKHR,
                                      dev: &Device,
                                      surface_format: vk::SurfaceFormatKHR)
                                      -> (Vec<vk::Image>, Vec<vk::ImageView>)
    {
        let images = swapchain_loader
            .get_swapchain_images(swapchain)
            .unwrap();

        let image_views = images.iter()
            .map(|&image| {
                // we want to interact with this image as a 2D
                // array of RGBA pixels (i.e. the "normal" way)
                let create_info = vk::ImageViewCreateInfo::builder()
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(surface_format.format)
                    // select the normal RGBA type
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
                    .image(image);

                dev.create_image_view(&create_info, None).unwrap()
            })
            .collect();

        return (images, image_views);
    }

    // Returns an index into the array of memory types for the memory
    // properties
    //
    // Memory types specify the location and accessability of memory. Device
    // local memory is resident on the GPU, while host visible memory can be
    // read from the system side. Both of these are part of the
    // vk::MemoryPropertyFlags type.
    fn find_memory_type_index(props: &vk::PhysicalDeviceMemoryProperties,
                              reqs: &vk::MemoryRequirements,
                              flags: vk::MemoryPropertyFlags)
                              -> Option<u32>
    {
        // for each memory type
        for (i, ref mem_type) in props.memory_types.iter().enumerate() {
            // Bit i of memoryBitTypes will be set if the resource supports
            // the ith memory type in props.
            //
            // ash autogenerates common operations for bitfield style structs
            // they can be found in `vk_bitflags_wrapped`
            if (reqs.memory_type_bits >> i) & 1 == 1
                && mem_type.property_flags.contains(flags) {
                    // log!(LogLevel::profiling, "Selected type with flags {:?}",
                    //          mem_type.property_flags);
                    // return the index into the memory type array
                    return Some(i as u32);
            }
        }
        None
    }

    // Create a vkImage and the resources needed to use it
    //   (vkImageView and vkDeviceMemory)
    //
    // Images are generic buffers which can be used as sources or
    // destinations of data. Images are accessed through image views,
    // which specify how the image will be modified or read. In vulkan
    // memory management is more hands on, so we will allocate some device
    // memory to back the image.
    //
    // This method may require some adjustment as it makes some assumptions
    // about the type of image to be created.
    //
    // Resolution should probably be the same size as the swapchain's images
    // usage defines the role the image will serve (transfer, depth data, etc)
    // flags defines the memory type (probably DEVICE_LOCAL + others)
    unsafe fn create_image(dev: &Device,
                           mem_props: &vk::PhysicalDeviceMemoryProperties,
                           resolution: &vk::Extent2D,
                           format: vk::Format,
                           usage: vk::ImageUsageFlags,
                           aspect: vk::ImageAspectFlags,
                           flags: vk::MemoryPropertyFlags)
                           -> (vk::Image, vk::ImageView, vk::DeviceMemory)
    {
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
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let image = dev.create_image(&create_info, None).unwrap();

        // we need to find a memory type that matches the type our
        // new image needs
        let mem_reqs = dev.get_image_memory_requirements(image);
        let memtype_index =
            Renderer::find_memory_type_index(mem_props,
                                             &mem_reqs,
                                             flags).unwrap();

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
                    .build()
            )
            .image(image)
            .format(create_info.format)
            .view_type(vk::ImageViewType::TYPE_2D);

        let view = dev.create_image_view(&view_info, None).unwrap();

        return (image, view, image_memory);
    }

    // Create an image sampler
    //
    // Samplers are used to filter data from an image when
    // it is referenced from a fragment shader. It allows
    // for additional processing effects on the input.
    unsafe fn create_sampler(&self) -> vk::Sampler {
        let info = vk::SamplerCreateInfo::builder()
            // filter for magnified (oversampled) pixels
            .mag_filter(vk::Filter::LINEAR)
            // filter for minified (undersampled) pixels
            .min_filter(vk::Filter::LINEAR)
            // repeat the texture on wraparound
            .address_mode_u(vk::SamplerAddressMode::REPEAT)
            .address_mode_v(vk::SamplerAddressMode::REPEAT)
            .address_mode_w(vk::SamplerAddressMode::REPEAT)
            // disable this for performance
            .anisotropy_enable(false)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
            // texture coords are [0,1)
            .unnormalized_coordinates(false)
            .compare_enable(false)
            .compare_op(vk::CompareOp::ALWAYS)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR);

        self.dev.create_sampler(&info, None).unwrap()
    }

    // Transitions `image` to the `new` layout using `cbuf`
    //
    // Images need to be manually transitioned from two layouts. A
    // normal use case is transitioning an image from an undefined
    // layout to the optimal shader access layout. This is also
    // used  by depth images.
    //
    // It is assumed this is for textures referenced from the fragment
    // shader, and so it is a bit specific.
    unsafe fn transition_image_layout(&self,
                                      image: vk::Image,
                                      cbuf: vk::CommandBuffer,
                                      old: vk::ImageLayout,
                                      new: vk::ImageLayout)
    {
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

    // Copies a widthxheight buffer to an image
    //
    // This is used to load a texture into an image
    // to be sampled by the shaders. The buffer will
    // usually be a staging buffer, see
    // `create_image_with_contents` for an example.
    //
    // needs to be recorded in a cbuf
    unsafe fn copy_buf_to_img(&self,
                              cbuf: vk::CommandBuffer,
                              buffer: vk::Buffer,
                              image: vk::Image,
                              width: u32,
                              height: u32)
    {
        let region = vk::BufferImageCopy::builder()
            // 0 specifies that the pixels are tightly packed
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(vk::ImageSubresourceLayers::builder()
                               .aspect_mask(vk::ImageAspectFlags::COLOR)
                               .mip_level(0)
                               .base_array_layer(0)
                               .layer_count(1)
                               .build()
            )
            .image_offset(vk::Offset3D {
                x: 0,
                y: 0,
                z: 0
            })
            .image_extent(vk::Extent3D {
                width: width,
                height: height,
                depth: 1,
            })
            .build();

        self.dev.cmd_copy_buffer_to_image(
            cbuf,
            buffer,
            image,
            // this is the layout the image is currently using
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[region]
        );
    }

    // Returns true if there are any resources in
    // the current release list.
    pub fn release_is_empty(&mut self) -> bool {
        return self.r_release.is_empty();
    }

    // Drop all of the resources, this is used to
    // release wl_buffers after they have been drawn.
    // We should not deal with wayland structs
    // directly, just with releaseinfo
    pub fn release_pending_resources(&mut self) {
        log!(LogLevel::profiling, "releasing pending resources --> {:?}",
             self.r_release);
        // This is the previous frames's pending release list
        // We will clear it, therefore dropping all the relinfos
        self.r_release.clear();
    }

    // Add a ReleaseInfo to the list of resources to be
    // freed this frame
    //
    // Takes care of choosing what list to add info to
    pub fn register_for_release(&mut self,
                                release: ReleaseInfo)
    {
       self.r_release.push(release);
    }

    // Find an app's mesh and update its contents
    pub fn update_app_contents(&mut self,
                               app: &mut App,
                               data: WindowContents,
                               release: ReleaseInfo)
    {
        app.mesh.as_mut().map(|mesh| {
            mesh.update_contents(self, data, release);
        });
    }

    // Update an image from a VkBuffer
    //
    // It is common to copy host data into an image
    // to initialize it. This function initializes
    // image by copying buffer to it.
    unsafe fn update_image_contents_from_buf(&mut self,
                                             buffer: vk::Buffer,
                                             image: vk::Image,
                                             width: u32,
                                             height: u32)
    {
        if let Some(ctx) = &mut *self.app_ctx.borrow_mut() {
            // If a previous copy is still happening, wait for it
            match self.dev.get_fence_status(ctx.copy_cbuf_fence) {
                // true means vk::Result::SUCCESS
                Ok(true) => {},
                // false means vk::Result::NOT_READY
                Ok(false) => {
                    self.dev.wait_for_fences(&[ctx.copy_cbuf_fence],
                                             true, // wait for all
                                             std::u64::MAX, //timeout
                    ).unwrap();
                    // unsignal it, may be extraneous
                    self.dev.reset_fences(&[ctx.copy_cbuf_fence]).unwrap();
                }
                Err(_) => panic!("Failed to get fence status"),
            };

            // now perform the copy
            self.cbuf_begin_recording(
                ctx.copy_cbuf,
                vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT
            );

            // transition our image to be a transfer destination
            self.transition_image_layout(
                image,
                ctx.copy_cbuf,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );

            self.copy_buf_to_img(ctx.copy_cbuf,
                                 buffer,
                                 image,
                                 width,
                                 height);

            // transition back to the optimal color format
            self.transition_image_layout(
                image,
                ctx.copy_cbuf,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );

            self.cbuf_end_recording(ctx.copy_cbuf);
            self.cbuf_submit_async(
                ctx.copy_cbuf,
                self.present_queue,
                &[], // wait_stages
                &[], // wait_semas
                &[], // signal_semas
                ctx.copy_cbuf_fence,
            );
        }
    }

    // Create a new image, and fill it with `data`
    //
    // This is meant for loading a texture into an image.
    // It essentially just wraps `create_image` and
    // `update_memory`.
    //
    // The resulting image will be in the shader read layout
    unsafe fn create_image_with_contents(
        &mut self,
        resolution: &vk::Extent2D,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect_flags: vk::ImageAspectFlags,
        mem_flags: vk::MemoryPropertyFlags,
        src_buf: vk::Buffer)
        -> (vk::Image, vk::ImageView, vk::DeviceMemory)
    {
        let (image, view, img_mem) = Renderer::create_image(&self.dev,
                                                            &self.mem_props,
                                                            resolution,
                                                            format,
                                                            usage,
                                                            aspect_flags,
                                                            mem_flags);

        self.update_image_contents_from_buf(
            src_buf,
            image,
            resolution.width,
            resolution.height,
        );

        (image, view, img_mem)
    }

    // Create a new Vulkan Renderer
    //
    // This renderer is very application specific. It is not meant to be
    // a generic safe wrapper for vulkan. This method constructs a new context,
    // creating a vulkan instance, finding a physical gpu, setting up a logical
    // device, and creating a swapchain.
    //
    // All methods called after this only need to take a mutable reference to
    // self, avoiding any nasty argument lists like the functions above. 
    // The goal is to have this make dealing with the api less wordy.
    pub fn new() -> Renderer {
        unsafe {
            let (entry, inst) = Renderer::create_instance();
            
            let (dr_loader, d_callback) = Renderer::setup_debug(&entry,
                                                                &inst);

            let pdev = Renderer::select_pdev(&inst);

            // Our display is in charge of choosing a medium to draw on,
            // and will create a surface on that medium
            let display = Display::new(&entry, &inst, pdev);

            let graphics_queue_family =
                Renderer::select_queue_family(&inst,
                                              pdev,
                                              &display.surface_loader,
                                              display.surface,
                                              vk::QueueFlags::GRAPHICS);
            let transfer_queue_family =
                Renderer::select_queue_family(&inst,
                                              pdev,
                                              &display.surface_loader,
                                              display.surface,
                                              vk::QueueFlags::TRANSFER);
            let mem_props = Renderer::get_pdev_mem_properties(&inst, pdev);

            // do this after we have gotten a valid physical device
            let surface_format = display.select_surface_format(pdev);

            let surface_caps = display.surface_loader
                .get_physical_device_surface_capabilities(pdev,
                                                          display.surface)
                .unwrap();
            let surface_resolution = display.select_resolution(
                &surface_caps
            );
            log!(LogLevel::profiling, "Rendering with resolution {:?}",
                 surface_resolution);

            let dev = Renderer::create_device(&inst, pdev,
                                              &[graphics_queue_family]);
            let present_queue = dev.get_device_queue(graphics_queue_family, 0);
            let transfer_queue = dev.get_device_queue(transfer_queue_family, 0);

            let swapchain_loader = khr::Swapchain::new(&inst, &dev);
            let swapchain = Renderer::create_swapchain(
                &swapchain_loader,
                &display.surface_loader,
                pdev,
                display.surface,
                &surface_caps,
                surface_format,
                &surface_resolution
            );
            
            let (images, image_views) =
                Renderer::select_images_and_views(&swapchain_loader,
                                                  swapchain,
                                                  &dev,
                                                  surface_format);

            let pool = Renderer::create_command_pool(&dev, graphics_queue_family);
            let buffers = Renderer::create_command_buffers(&dev,
                                                           pool,
                                                           images.len() as u32);

            // the depth attachment needs to have its own resources
            let (depth_image, depth_image_view, depth_image_mem) =
                Renderer::create_image(
                    &dev,
                    &mem_props,
                    &surface_resolution,
                    vk::Format::D16_UNORM,
                    vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                    vk::ImageAspectFlags::DEPTH,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL
                );

            let sema_create_info = vk::SemaphoreCreateInfo::default();

            let present_sema = dev
                .create_semaphore(&sema_create_info, None)
                .unwrap();
            let render_sema = dev
                .create_semaphore(&sema_create_info, None)
                .unwrap();

            let fence = dev.create_fence(
                &vk::FenceCreateInfo::builder()
                    .flags(vk::FenceCreateFlags::SIGNALED),
                None,
            ).expect("Could not create fence");

            let ext_mem_loader = khr::ExternalMemoryFd::new(&inst, &dev);

            // you are now the proud owner of a half complete
            // rendering context
            Renderer {
                debug_loader: dr_loader,
                debug_callback: d_callback,
                loader: entry,
                inst: inst,
                dev: dev,
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
                images: images,
                views: image_views,
                depth_image: depth_image,
                depth_image_view: depth_image_view,
                depth_image_mem: depth_image_mem,
                pool: pool,
                cbufs: buffers,
                present_sema: present_sema,
                render_sema: render_sema,
                submit_fence: fence,
                app_ctx: RefCell::new(None),
                external_mem_fd_loader: ext_mem_loader,
                r_release: Vec::new(),
            }
        }
    }

    // Records and submits a one-time command buffer.
    //
    // cbuf - the command buffer to use
    // queue - the queue to submit cbuf to
    // wait_stages - a list of pipeline stages to wait on
    // wait_semas - semaphores we consume
    // signal_semas - semaphores we notify
    //
    // All operations in the `record_fn` argument will be
    // submitted in the command buffer `cbuf`. This aims to make
    // constructing buffers more ergonomic.
    fn cbuf_onetime<F: FnOnce(&Renderer, vk::CommandBuffer)>
        (&self,
         cbuf: vk::CommandBuffer,
         queue: vk::Queue,
         wait_stages: &[vk::PipelineStageFlags],
         wait_semas: &[vk::Semaphore],
         signal_semas: &[vk::Semaphore],
         record_fn: F)
    {
        self.cbuf_begin_recording(
            cbuf,
            vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT
        );

        record_fn(self, cbuf);

        self.cbuf_end_recording(cbuf);

        self.cbuf_submit(cbuf,
                         queue,
                         wait_stages,
                         wait_semas,
                         signal_semas);

        unsafe {
            // We need to wait for the command submission to finish, this
            // is why you should avoid using this function
            self.dev.wait_for_fences(&[self.submit_fence],
			             true, // wait for all
			             std::u64::MAX, //timeout
            ).unwrap();

            // do not reset the fence since the next cbuf_submit will
            // expect it to be signaled
        }
    }

    // Submits a command buffer.
    //
    // This is used for synchronized submits for graphical
    // display operations. It waits for submit_fence before
    // submitting to queue, and will signal it when the
    // cbuf is executed. (see cbuf_sumbmit_async)
    //
    // The buffer MUST have been recorded before this
    //
    // cbuf - the command buffer to use
    // queue - the queue to submit cbuf to
    // wait_stages - a list of pipeline stages to wait on
    // wait_semas - semaphores we consume
    // signal_semas - semaphores we notify
    fn cbuf_submit
        (&self,
         cbuf: vk::CommandBuffer,
         queue: vk::Queue,
         wait_stages: &[vk::PipelineStageFlags],
         wait_semas: &[vk::Semaphore],
         signal_semas: &[vk::Semaphore])
    {
        unsafe {
            // If the app context has been initialized,
            // then include the fence for copy operations
            let fences = match self.app_ctx
                .borrow_mut()
                .as_ref() {
                    Some(ctx) => vec![self.submit_fence,
                                      ctx.copy_cbuf_fence],
                    None => vec![self.submit_fence],
                };

            // Before we submit ourselves, we need to wait for the
            // previous frame's execution and any copy commands to finish
            self.dev.wait_for_fences(fences.as_slice(),
			             true, // wait for all
			             std::u64::MAX, //timeout
            ).unwrap();

            // we need to reset the fence since it has been signaled
            // copy fence will be handled elsewhere
            self.dev.reset_fences(&[self.submit_fence]).unwrap();

            self.cbuf_submit_async(cbuf,
                                   queue,
                                   wait_stages,
                                   wait_semas,
                                   signal_semas,
                                   self.submit_fence);
        }
    }

    // Submits a command buffer asynchronously.
    //
    // Simple wrapper for queue submission. Does not
    // wait for anything.
    //
    // The buffer MUST have been recorded before this
    //
    // cbuf - the command buffer to use
    // queue - the queue to submit cbuf to
    // wait_stages - a list of pipeline stages to wait on
    // wait_semas - semaphores we consume
    // signal_semas - semaphores we notify
    fn cbuf_submit_async
        (&self,
         cbuf: vk::CommandBuffer,
         queue: vk::Queue,
         wait_stages: &[vk::PipelineStageFlags],
         wait_semas: &[vk::Semaphore],
         signal_semas: &[vk::Semaphore],
         signal_fence: vk::Fence)
    {
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
            self.dev.queue_submit(
                queue,
                &[submit_info],
                signal_fence,
            ).unwrap();
        }
    }

    // Records but does not submit a command buffer.
    //
    // cbuf - the command buffer to use
    // flags - the usage flags for the buffer
    //
    // All operations in the `record_fn` argument will be
    // recorded in the command buffer `cbuf`.
    fn cbuf_begin_recording(&self,
                            cbuf: vk::CommandBuffer,
                            flags: vk::CommandBufferUsageFlags)
    {
        unsafe {
            // first reset the queue so we know it is empty
            self.dev.reset_command_buffer(
                cbuf,
                vk::CommandBufferResetFlags::RELEASE_RESOURCES,
            ).expect("Could not reset command buffer");

            // this cbuf will only be used once, so tell vulkan that
            // so it can optimize accordingly
            let record_info = vk::CommandBufferBeginInfo::builder()
                .flags(flags);

            // start recording the command buffer, call the function
            // passed to load it with operations, and then end the
            // command buffer
            self.dev.begin_command_buffer(cbuf, &record_info)
                .expect("Could not start command buffer");
        }
    }

    
    // Records but does not submit a command buffer.
    //
    // cbuf - the command buffer to use
    fn cbuf_end_recording(&self, cbuf: vk::CommandBuffer) {
        unsafe {
            self.dev.end_command_buffer(cbuf)
                .expect("Could not end command buffer");
        }
    }

    pub fn get_recording_parameters(&self) -> RecordParams{
        RecordParams {
            cbuf: self.cbufs[self.current_image as usize],
            image_num: self.current_image as usize,
        }
    }

    // Start recording a cbuf for one frame
    //
    // Each framebuffer has a set of resources, including command
    // buffers. This records the cbufs for the framebuffer
    // specified by `img`.
    pub fn begin_recording_one_frame(&mut self,
                                     params: &RecordParams)
    {
        unsafe {
            // we need to clear any existing data when we start a pass
            let clear_vals = [
                vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [0.0, 0.0, 0.0, 0.0],
                    },
                },
                vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                },
            ];

            // Most of the resources we use are app specific
            if let Some(ctx) = &*self.app_ctx.borrow() {
                // We want to start a render pass to hold all of
                // our drawing. The actual pass is started in the cbuf
                let pass_begin_info = vk::RenderPassBeginInfo::builder()
                    .render_pass(ctx.pass)
                    .framebuffer(ctx.framebuffers[params.image_num])
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: self.resolution,
                    })
                    .clear_values(&clear_vals);

                // start the cbuf
                self.cbuf_begin_recording(
                    params.cbuf,
                    vk::CommandBufferUsageFlags::SIMULTANEOUS_USE
                );

                // -- Setup static drawing resources
                // All of our drawing operations need
                // to be recorded inside a render pass.
                self.dev.cmd_begin_render_pass(
                    params.cbuf,
                    &pass_begin_info,
                    vk::SubpassContents::INLINE,
                );

                self.dev.cmd_bind_pipeline(
                    params.cbuf,
                    vk::PipelineBindPoint::GRAPHICS,
                    ctx.pipeline
                );

                // bind the vertex and index buffers from
                // the first mesh
                self.dev.cmd_bind_vertex_buffers(
                    params.cbuf, // cbuf to draw in
                    0, // first vertex binding updated by the command
                    &[ctx.vert_buffer], // set of buffers to bind
                    &[0], // offsets for the above buffers
                );
                self.dev.cmd_bind_index_buffer(
                    params.cbuf,
                    ctx.index_buffer,
                    0, // offset
                    vk::IndexType::UINT32,
                );
            }
        }
    }

    // Stop recording a cbuf for one frame
    pub fn end_recording_one_frame(&mut self, params: &RecordParams) {
        unsafe {
            self.dev.cmd_end_render_pass(params.cbuf);
            self.cbuf_end_recording(params.cbuf);
        }
    }

    // set up the depth image in self.
    //
    // We need to transfer the format of the depth image to something
    // usable. We will use an image barrier to set the image as a depth
    // stencil attachment to be used later.
    unsafe fn setup_depth_image(&mut self) {
        // allocate a new cbuf for us to work with
        let new_cbuf = Renderer::create_command_buffers(&self.dev,
                                                        self.pool,
                                                        1)[0]; // only get one

        // the depth image and view have already been created by new
        // we need to execute a cbuf to set up the memory we are
        // going to use later
        self.cbuf_onetime(
            new_cbuf,
            self.present_queue,
            &[], // wait_stages
            &[], // wait_semas
            &[], // signal_semas
            // this closure will be the contents of the cbuf
            |rend, cbuf| {
                // We need to initialize the depth attachment by
                // performing a layout transition to the optimal
                // depth layout
                //
                // we do not use rend.transition_image_layout since that
                // is specific to texture images
                let layout_barrier = vk::ImageMemoryBarrier::builder()
                    .image(rend.depth_image)
                    // access patern for the resulting layout
                    .dst_access_mask(
                        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
                    )
                    // go from an undefined old layout to whatever the
                    // driver decides is the optimal depth layout
                    .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .subresource_range(
                        vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::DEPTH)
                            .layer_count(1)
                            .level_count(1)
                            .build(),
                    )
                    .build();

                // process the barrier we created, which will perform
                // the actual transition.
                rend.dev.cmd_pipeline_barrier(
                    cbuf,
                    vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                    vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[layout_barrier],
                );
            },
        );
    }

    // create a renderpass for the color/depth attachments
    //
    // Render passses signify what attachments are used in which
    // stages. They are composed of one or more subpasses.
    unsafe fn create_pass(&mut self) -> vk::RenderPass {
        let attachments = [
            // the color dest. Its the surface we slected in new
            vk::AttachmentDescription {
                format: self.surface_format.format,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: vk::AttachmentLoadOp::CLEAR,
                store_op: vk::AttachmentStoreOp::STORE,
                final_layout: vk::ImageLayout::PRESENT_SRC_KHR,
                ..Default::default()
            },
            // the depth attachment
            vk::AttachmentDescription {
                format: vk::Format::D16_UNORM,
                samples: vk::SampleCountFlags::TYPE_1,
                load_op: vk::AttachmentLoadOp::CLEAR,
                initial_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                final_layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
                ..Default::default()
            },
        ];

        // identify which of the above attachments
        let color_refs = [ vk::AttachmentReference {
            attachment: 0, // index into the attachments variable
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let depth_refs = vk::AttachmentReference {
            attachment: 1,
            layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };

        // our subpass isn't dependent on anything, and it writes to color output
        let dependencies = [ vk::SubpassDependency {
            src_subpass: vk::SUBPASS_EXTERNAL,
            src_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_READ
                | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            dst_stage_mask: vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            ..Default::default()
        }];

        // our render pass only has one subpass, which only does graphical ops
        let subpasses = [vk::SubpassDescription::builder()
                         .color_attachments(&color_refs)
                         .depth_stencil_attachment(&depth_refs)
                         .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                         .build()
        ];

        let create_info = vk::RenderPassCreateInfo::builder()
            .attachments(&attachments)
            .subpasses(&subpasses)
            .dependencies(&dependencies);

        self.dev.create_render_pass(&create_info, None).unwrap()
    }

    // Create a vkShaderModule for one of the dynamic pipeline stages
    //
    // dynamic portions of the graphics pipeline are programmable with
    // spirv code. This helper function accepts a file name (`cursor`) and
    // creates a shader module from it.
    //
    // `cursor` is accepted by ash's helper function, `read_spv`
    unsafe fn create_shader_module(&mut self, cursor:
                                   &mut Cursor<&'static [u8]>)
                                   -> vk::ShaderModule
    {
        let code = util::read_spv(cursor)
            .expect("Could not read spv file");

        let info = vk::ShaderModuleCreateInfo::builder()
            .code(&code);

        self.dev.create_shader_module(&info, None)
            .expect("Could not create new shader module")
    }

    // Create the dynamic portions of the rendering pipeline
    //
    // Shader stages specify the usage of a shader module, such as the
    // entrypoint name (usually main) and the type of shader. As of now,
    // we only return two shader modules, vertex and fragment.
    //
    // `entrypoint`: should be a CString.as_ptr(). The CString that it
    // represents should live as long as the return type of this method.
    //  see: https://doc.rust-lang.org/std/ffi/struct.CString.html#method.as_ptr
    unsafe fn create_shader_stages(&mut self, entrypoint: *const i8)
                                 -> [vk::PipelineShaderStageCreateInfo; 2]
    {
        let vert_shader = self.create_shader_module(
            &mut Cursor::new(&include_bytes!("./shaders/vert.spv")[..])
        );
        let frag_shader = self.create_shader_module(
            &mut Cursor::new(&include_bytes!("./shaders/frag.spv")[..])
        );

        // note that the return size is 2 elements to match the return type
        [
            vk::PipelineShaderStageCreateInfo {
                module: vert_shader,
                p_name: entrypoint,
                stage: vk::ShaderStageFlags::VERTEX,
                ..Default::default()
            },
            vk::PipelineShaderStageCreateInfo {
                module: frag_shader,
                p_name: entrypoint,
                stage: vk::ShaderStageFlags::FRAGMENT,
                ..Default::default()
            },
        ]
    }

    // Configure and create a graphics pipeline
    //
    // In vulkan, the programmer has explicit control over the format
    // and layout of the entire graphical pipeline, both dynamic and
    // fixed function portions. We will specify the vertex input, primitive
    // assembly, viewport/stencil location, rasterization type, depth
    // information, and color blending.
    //
    // Pipeline layouts specify the full set of resources that the pipeline
    // can access while running.
    //
    // This method roughly follows the "fixed function" part of the
    // vulkan tutorial.
    unsafe fn create_pipeline(&mut self,
                              layout: vk::PipelineLayout,
                              pass: vk::RenderPass,
                              shader_stages:
                              &[vk::PipelineShaderStageCreateInfo])
                              -> vk::Pipeline
    {
        // This binds our vertex input to location 0 to be passed to the shader
        // Think of it like specifying the data stream given to the shader
        let vertex_bindings = [vk::VertexInputBindingDescription {
            binding: 0, // (location = 0)
            stride: mem::size_of::<VertData>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }];

        // These describe how the shader should parse the data passed
        // think of it like breaking the above data stream into variables
        let vertex_attributes = [
            // vertex location
            vk::VertexInputAttributeDescription {
                binding: 0, // The data binding to parse
                location: 0, // the location of the attribute we are specifying
                // Common types
                //     float: VK_FORMAT_R32_SFLOAT
                //     vec2:  VK_FORMAT_R32G32_SFLOAT
                //     vec3:  VK_FORMAT_R32G32B32_SFLOAT
                //     vec4:  VK_FORMAT_R32G32B32A32_SFLOAT
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(VertData, vertex) as u32,
            },
            // Texture coordinates
            vk::VertexInputAttributeDescription {
                binding: 0, // The data binding to parse
                location: 1, // the location of the attribute we are specifying
                format: vk::Format::R32G32_SFLOAT,
                offset: offset_of!(VertData, tex) as u32,
            },
        ];

        // now for the fixed function portions of the pipeline
        // This describes the layout of resources passed to the shaders
        let vertex_info = vk::PipelineVertexInputStateCreateInfo::builder()
            .vertex_binding_descriptions(&vertex_bindings)
            .vertex_attribute_descriptions(&vertex_attributes);

        // input assembly describes how to turn the vertex
        // and index buffers into primatives
        let assembly = vk::PipelineInputAssemblyStateCreateInfo {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            ..Default::default()
        };

        // will almost always be (0,0) with size (width, height)
        let viewport = [vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.resolution.width as f32,
            height: self.resolution.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        }];
        // no scissor test
        let scissor = [vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: self.resolution,
        }];

        let viewport_info = vk::PipelineViewportStateCreateInfo::builder()
            .scissors(&scissor)
            .viewports(&viewport);

        // we want the normal counter-clockwise vertices, and filled in polys
        let raster_info = vk::PipelineRasterizationStateCreateInfo {
            front_face: vk::FrontFace::CLOCKWISE,
            line_width: 1.0,
            polygon_mode: vk::PolygonMode::FILL,
            ..Default::default()
        };

        // combines all of the fragments found at a pixel for anti-aliasing
        // just disable this
        let multisample_info = vk::PipelineMultisampleStateCreateInfo {
            rasterization_samples: vk::SampleCountFlags::TYPE_1,
            ..Default::default()
        };

        // no stencil operations, so this just keeps everything
        let stencil_state = vk::StencilOpState {
            fail_op: vk::StencilOp::KEEP,
            pass_op: vk::StencilOp::KEEP,
            depth_fail_op: vk::StencilOp::KEEP,
            compare_op: vk::CompareOp::ALWAYS,
            ..Default::default()
        };
        
        // we do want a depth test enabled for this, using our noop stencil
        // test. This should record Z-order to 1,000
        let depth_info = vk::PipelineDepthStencilStateCreateInfo {
            depth_test_enable: 1,
            depth_write_enable: 1,
            depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
            front: stencil_state,
            back: stencil_state,
            max_depth_bounds: 1.0,
            ..Default::default()
        };

        // just do basic alpha blending. This is straight from the tutorial
        let blend_attachment_states = [vk::PipelineColorBlendAttachmentState {
            blend_enable: 1, // VK_TRUE
            // blend the new contents over the old
            src_color_blend_factor: vk::BlendFactor::SRC_ALPHA,
            dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            color_blend_op: vk::BlendOp::ADD,
            src_alpha_blend_factor: vk::BlendFactor::ONE,
            dst_alpha_blend_factor: vk::BlendFactor::ZERO,
            alpha_blend_op: vk::BlendOp::ADD,
            color_write_mask: vk::ColorComponentFlags::all(),
        }];

        let blend_info = vk::PipelineColorBlendStateCreateInfo::builder()
            .logic_op(vk::LogicOp::CLEAR)
            .attachments(&blend_attachment_states);

        // dynamic state specifies what parts of the pipeline will be
        // specified at draw time. (like moving the viewport)
        // we don't want any of that atm

        let pipeline_info = vk::GraphicsPipelineCreateInfo::builder()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_info)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport_info)
            .rasterization_state(&raster_info)
            .multisample_state(&multisample_info)
            .depth_stencil_state(&depth_info)
            .color_blend_state(&blend_info)
            .layout(layout)
            .render_pass(pass)
            .build();

        // Allocate one pipeline and return it
        self.dev.create_graphics_pipelines(
            vk::PipelineCache::null(),
            &[pipeline_info],
            None,
        ).expect("Could not create graphics pipeline")[0]
    }

    // Create framebuffers for each swapchain image
    //
    // Image views represent a portion of an allocated image, while
    // framebuffers bind an image view for use in a render pass. A
    // framebuffer is really just a collection of attachments.
    //
    // In our example, we pair color and depth attachments in our
    // framebuffers.
    unsafe fn create_framebuffers(&mut self,
                                  pass: vk::RenderPass,
                                  res: vk::Extent2D)
                                  -> Vec<vk::Framebuffer>
    {
        // A framebuffer should be created for each of the swapchain
        // images. Reuse the depth buffer for all images since it
        // doesn't change.
        self.views.iter()
            .map(|&view| {
                // color, depth
                let attachments = [
                    view, self.depth_image_view,
                ];

                let info = vk::FramebufferCreateInfo::builder()
                    .render_pass(pass)
                    .attachments(&attachments)
                    .width(res.width)
                    .height(res.height)
                    .layers(1);

                self.dev.create_framebuffer(&info, None)
                    .unwrap()
            })
            .collect()
    }

    // Returns a `ShaderConstants` with the default values for this application
    //
    // Constants will be the contents of the uniform buffers which are
    // processed by the shaders. The most obvious entry is the model + view
    // + perspective projection matrix.
    fn get_shader_constants(resolution: vk::Extent2D)
                            -> ShaderConstants
    {
        // transform from blender's coordinate system to vulkan
        let model = Matrix4::from_translation(Vector3::new(-1.0, -1.0, 0.0));

        ShaderConstants {
            model: model,
            width: resolution.width as f32,
            height: resolution.height as f32,
        }
    }

    // Create uniform buffer descriptor layout
    //
    // Descriptor layouts specify the number and characteristics of descriptor
    // sets which will be made available to the pipeline through the pipeline
    // layout.
    //
    // The layouts created will be the default for this application. This should
    // usually be at least one descriptor for the MVP martrix.
    unsafe fn create_ubo_layout(&mut self)
                                -> vk::DescriptorSetLayout
    {
        // supplies `descriptor_uniform_layouts`
        // ubos for the MVP matrix and image samplers for textures
        let bindings = [vk::DescriptorSetLayoutBinding::builder()
                        .binding(0)
                        .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                        .stage_flags(vk::ShaderStageFlags::VERTEX)
                        .descriptor_count(1)
                        .build(),
        ];

        let info = vk::DescriptorSetLayoutCreateInfo::builder()
            .bindings(&bindings);

        self.dev.create_descriptor_set_layout(&info, None)
            .unwrap()
    }

    // Create a descriptor pool for the uniform buffer
    //
    // All other dynamic sets are tracked using a DescPool. This pool
    // is for statically numbered resources.
    //
    // The pool returned is NOT thread safe
    unsafe fn create_descriptor_pool(&mut self)
                                     -> vk::DescriptorPool
    {
        let size = [vk::DescriptorPoolSize::builder()
                    .ty(vk::DescriptorType::UNIFORM_BUFFER)
                    .descriptor_count(1)
                    .build(),
        ];

        let info = vk::DescriptorPoolCreateInfo::builder()
            .pool_sizes(&size)
            .max_sets(1);

        self.dev.create_descriptor_pool(&info, None).unwrap()
    }

    // Allocate a descriptor set for each layout in `layouts`
    //
    // A descriptor set specifies a group of attachments that can
    // be referenced by the graphics pipeline. Think of a descriptor
    // as the hardware's handle to a resource. The set of descriptors
    // allocated in each set is specified in the layout.
    unsafe fn allocate_descriptor_sets(&self,
                                       pool: vk::DescriptorPool,
                                       layouts: &[vk::DescriptorSetLayout])
                                       -> Vec<vk::DescriptorSet>
    {
        let info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(pool)
            .set_layouts(layouts)
            .build();

        self.dev.allocate_descriptor_sets(&info).unwrap()
    }

    // Update a uniform buffer descriptor set with `buf`
    //
    // Update the entry in `set` at offset `element` to use the
    // values in `buf`. Descriptor sets can be updated outside of
    // command buffers.
    unsafe fn update_uniform_descriptor_set(&mut self,
                                            buf: vk::Buffer,
                                            set: vk::DescriptorSet,
                                            binding: u32,
                                            element: u32)
    {
        let info = vk::DescriptorBufferInfo::builder()
            .buffer(buf)
            .offset(0)
            .range(mem::size_of::<ShaderConstants>() as u64)
            .build();
        let write_info = [
            vk::WriteDescriptorSet::builder()
                .dst_set(set)
                .dst_binding(binding)
                // descriptors can be arrays, so we need to specify an offset
                // into that array if applicable
                .dst_array_element(element)
                .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                .buffer_info(&[info])
                .build()
        ];

        self.dev.update_descriptor_sets(
            &write_info, // descriptor writes
            &[], // descriptor copies
        );
    }

    // Update an image sampler descriptor set
    //
    // This is what actually sets the image that the sampler
    // will filter for the shader. The image is referenced
    // by the `view` argument.
    unsafe fn update_sampler_descriptor_set(&self,
                                            set: vk::DescriptorSet,
                                            binding: u32,
                                            element: u32,
                                            sampler: vk::Sampler,
                                            view: vk::ImageView)
    {
        let info = vk::DescriptorImageInfo::builder()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(view)
            .sampler(sampler)
            .build();
        let write_info = [
            vk::WriteDescriptorSet::builder()
                .dst_set(set)
                .dst_binding(binding)
                // descriptors can be arrays, so we need to specify an offset
                // into that array if applicable
                .dst_array_element(element)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&[info])
                .build()
        ];

        self.dev.update_descriptor_sets(
            &write_info, // descriptor writes
            &[], // descriptor copies
        );
    }

    // Create vertex/index buffers for the default quad
    //
    // All onscreen regions will be represented by a quad, and
    // we only need to create one set of vertex/index buffers
    // for it.
    unsafe fn create_default_geom_bufs(&self)
                                       -> (vk::Buffer, vk::DeviceMemory,
                                           vk::Buffer, vk::DeviceMemory)
    {
        let (vbuf, vmem) = self.create_buffer(
            vk::BufferUsageFlags::VERTEX_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::HOST_VISIBLE
                | vk::MemoryPropertyFlags::HOST_COHERENT,
            &QUAD_DATA,
        );
        let (ibuf, imem) = self.create_buffer(
            vk::BufferUsageFlags::INDEX_BUFFER,
            vk::SharingMode::EXCLUSIVE,
            vk::MemoryPropertyFlags::HOST_VISIBLE
                | vk::MemoryPropertyFlags::HOST_COHERENT,
            &QUAD_INDICES,
        );

        return (vbuf, vmem, ibuf, imem);
    }

    // Create descriptors for the image samplers
    //
    // Each Mesh will have a descriptor for each framebuffer,
    // since multiple frames will be in flight. This allocates
    // `image_count` sampler descriptors.
    unsafe fn create_sampler_descriptors(&self,
                                         pool: vk::DescriptorPool,
                                         layout: vk::DescriptorSetLayout,
                                         image_count: u32)
                                         -> (vk::Sampler,
                                             Vec<vk::DescriptorSet>)
    {
        // One image sampler is going to be used for everything
        let sampler = self.create_sampler();
        // A descriptor needs to be created for every swapchaing image
        // so we can prepare the next frame while the current one is
        // processing.
        let mut descriptors = Vec::new();

        for _ in 0..image_count {
            let set = self.allocate_descriptor_sets(
                pool,
                &[layout]
            )[0];

            descriptors.push(set);
        }

        return (sampler, descriptors);
    }

    // Set up the application. This should *always* be called
    //
    // Once we have allocated a renderer with `new`, we should initialize
    // the rendering pipeline so that we can display things. This method
    // basically sets up all of the "application" specific resources like
    // shaders, geometry, and the like.
    //
    // This fills in the AppContext struct in the Renderer
    pub fn setup(&mut self) {
        unsafe {
            self.setup_depth_image();
            
            let pass = self.create_pass();
            
            // This is a really annoying issue with CString ptrs
            let program_entrypoint_name = CString::new("main").unwrap();
            // If the CString is created in `create_shaders`, and is inserted in
            // the return struct using the `.as_ptr()` method, then the CString
            // will still be dropped on return and our pointer will be garbage.
            // Instead we need to ensure that the CString will live long
            // enough. I have no idea why it is like this.
            let shader_stages = Box::new(
                self.create_shader_stages(program_entrypoint_name.as_ptr())
            );

            // Each window is going to have a sampler descriptor for every
            // framebuffer image. Unfortunately this means the descriptor
            // count will be runtime dependent.
            // This is an allocator for those descriptors
            let descpool = DescPool::create(&self);

            // prepare descriptors for all of the uniforms to pass to shaders
            //
            // NOTE: These need to be referenced in order by the `set` modifier
            // in the shaders
            let ubo_layout = self.create_ubo_layout();
            // These are the layout recognized by the pipeline
            let descriptor_layouts = &[
                ubo_layout,      // set 0
                descpool.layout, // set 1
            ];

            // make a push constant entry for the z ordering of a window
            let constants = &[vk::PushConstantRange::builder()
                              .stage_flags(vk::ShaderStageFlags::VERTEX)
                              .offset(0)
                              // depth is measured as a normalized float
                              .size(std::mem::size_of::<PushConstants>() as u32)
                              .build()];

            // even though we don't have anything special in our layout, we
            // still need to have a created layout for the pipeline
            let layout_info = vk::PipelineLayoutCreateInfo::builder()
                .push_constant_ranges(constants)
                .set_layouts(descriptor_layouts);
            let layout = self.dev.create_pipeline_layout(&layout_info, None)
                .unwrap();
            
            let pipeline = self.create_pipeline(layout, pass, &*shader_stages);

            let framebuffers = self.create_framebuffers(pass, self.resolution);

            // Allocate a pool only for the ubo descriptors
            let uniform_pool = self.create_descriptor_pool();
            let ubo = self.allocate_descriptor_sets(
                uniform_pool,
                &[ubo_layout],
            )[0];

            let consts = Renderer::get_shader_constants(self.resolution);

            // create a uniform buffer
            let (buf, mem) = self.create_buffer(
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                vk::SharingMode::EXCLUSIVE,
                vk::MemoryPropertyFlags::HOST_VISIBLE
                    | vk::MemoryPropertyFlags::HOST_COHERENT,
                // this specifies the constants to copy into the buffer
                &[consts],
            );

            // now we need to update the descriptor set with the
            // buffer of the uniform constants to use
            self.update_uniform_descriptor_set(
                buf,
                ubo,
                0, // binding
                0, // element
            );

            // Allocate buffers for all geometry to be used
            let (vbuf, vmem, ibuf, imem) = self.create_default_geom_bufs();

            // One image sampler is going to be used for everything
            let sampler = self.create_sampler();

            // Create a cbuf for copying data to shm images
            let copy_cbuf = Renderer::create_command_buffers(&self.dev,
                                                             self.pool,
                                                             1)[0];

            // Make a fence which will be signalled after
            // copies are completed
            let copy_fence = self.dev.create_fence(
                &vk::FenceCreateInfo::builder()
                    .flags(vk::FenceCreateFlags::SIGNALED),
                None,
            ).expect("Could not create fence");

            // The app context contains the scene specific data
            self.app_ctx = RefCell::new(Some(AppContext {
                pass: pass,
                pipeline: pipeline,
                pipeline_layout: layout,
                descriptor_uniform_layout: ubo_layout,
                framebuffers: framebuffers,
                uniform_buffer: buf,
                uniform_buffers_memory: mem,
                image_sampler: sampler,
                uniform_pool: uniform_pool,
                desc_pool: descpool,
                ubo_descriptor: ubo,
                shader_modules: shader_stages
                    .iter()
                    .map(|info| { info.module })
                    .collect(),
                vert_buffer: vbuf,
                vert_buffer_memory: vmem,
                // multiply the index len by the vector size
                vert_count: QUAD_INDICES.len() as u32 * 3,
                index_buffer: ibuf,
                index_buffer_memory: imem,
                copy_cbuf: copy_cbuf,
                copy_cbuf_fence: copy_fence,
            }));
        }
    }

    // Allocates a buffer/memory pair of size `size`.
    //
    // This is just a helper for `create_buffer`. It does not fill
    // the buffer with anything.
    unsafe fn create_buffer_with_size(&self,
                                      usage: vk::BufferUsageFlags,
                                      mode: vk::SharingMode,
                                      flags: vk::MemoryPropertyFlags,
                                      size: u64)
                                      -> (vk::Buffer, vk::DeviceMemory)
    {
        let create_info = vk::BufferCreateInfo::builder()
            .size(size)
            .usage(usage)
            .sharing_mode(mode);

        let buffer = self.dev.create_buffer(&create_info, None).unwrap();
        let req = self.dev.get_buffer_memory_requirements(buffer);
        // get the memory types for this pdev
        let props = Renderer::get_pdev_mem_properties(&self.inst, self.pdev);
        // find the memory type that best suits our requirements
        let index = Renderer::find_memory_type_index(
            &props,
            &req,
            flags,
        ).unwrap();

        // now we need to allocate memory to back the buffer
        let alloc_info = vk::MemoryAllocateInfo {
            allocation_size: req.size,
            memory_type_index: index,
            ..Default::default()
        };

        let memory = self.dev.allocate_memory(&alloc_info, None).unwrap();

        return (buffer, memory);
    }

    // Wrapper for freeing device memory
    //
    // Having this in one place lets us quickly handle any additional
    // allocation tracking
    unsafe fn free_memory(&self, mem: vk::DeviceMemory) {
        self.dev.free_memory(mem, None);
    }

    // Writes `data` to `memory`
    //
    // This is a helper method for mapping and updating the value stored
    // in device memory Memory needs to be host visible and coherent.
    // This does not flush after writing.
    unsafe fn update_memory<T: Copy>(&self,
                                     memory: vk::DeviceMemory,
                                     data: &[T])
    {
        // Now we copy our data into the buffer
        let data_size = std::mem::size_of_val(data) as u64;
        let ptr = self.dev.map_memory(
            memory,
            0, // offset
            data_size,
            vk::MemoryMapFlags::empty()
        ).unwrap();

        // rust doesn't have a raw memcpy, so we need to transform the void
        // ptr to a slice. This is unsafe as the length needs to be correct
        let dst = std::slice::from_raw_parts_mut(ptr as *mut T, data.len());
        dst.copy_from_slice(data);

        self.dev.unmap_memory(memory);
    }

    // allocates a buffer/memory pair and fills it with `data`
    //
    // There are two components to a memory backed resource in vulkan:
    // vkBuffer which is the actual buffer itself, and vkDeviceMemory which
    // represents a region of allocated memory to hold the buffer contents.
    //
    // Both are returned, as both need to be destroyed when they are done.
    unsafe fn create_buffer<T: Copy>(&self,
                                     usage: vk::BufferUsageFlags,
                                     mode: vk::SharingMode,
                                     flags: vk::MemoryPropertyFlags,
                                     data: &[T])
                                     -> (vk::Buffer, vk::DeviceMemory)
    {
        let size = std::mem::size_of_val(data) as u64;
        let (buffer, memory) = self.create_buffer_with_size(
            usage,
            mode,
            flags,
            size,
        );

        self.update_memory(memory, data);

        // Until now the buffer has not had any memory assigned
        self.dev.bind_buffer_memory(buffer, memory, 0).unwrap();

        (buffer, memory)
    }

    // Create a mesh and its needed data
    //
    // Meshes need to be in an indexed vertex format.
    //
    // tex_res is the resolution of `texture`
    // window_res is the size of the on screen window
    pub fn create_mesh(&mut self,
                       data: WindowContents,
                       release: ReleaseInfo)
                       -> Option<Mesh>
    {
        return Mesh::new(self, data, release);
    }

    // Apply a transform matrix to all meshes
    //
    // This updates the model matrix of the shader constants
    // used for all models
    pub fn transform_meshes(&mut self,
                            transform: &Matrix4<f32>)
    {
        let mut consts = Renderer::get_shader_constants(self.resolution);
        consts.model = consts.model * transform;

        unsafe {
            if let Some(ctx) = &*self.app_ctx.borrow() {
                self.update_memory(ctx.uniform_buffers_memory, &[consts]);
            }
        }
    }

    // Update self.current_image with the swapchain image to render to
    //
    // Returns if the next image index was successfully obtained
    // false means try again later, the next image is not ready
    pub fn get_next_swapchain_image(&mut self) -> bool {
        unsafe {
            match self.swapchain_loader.acquire_next_image(
                self.swapchain,
                std::u64::MAX, // use a zero timeout to immediately get the state
                self.present_sema, // signals presentation
                vk::Fence::null())
            {
                // TODO: handle suboptimal surface regeneration
                Ok((index, _)) => {
                    self.current_image = index;
                    return true;
                },
                Err(vk::Result::NOT_READY) => return false,
                Err(vk::Result::TIMEOUT) => return false,
                // the call did not succeed
                Err(err) =>
                    panic!("Could not acquire next image: {:?}", err),
            };
        }
    }

    // Render a frame, but do not present it
    //
    // Think of this as the "main" rendering operation. It will draw
    // all geometry to the current framebuffer. Presentation is
    // done later, in case operations need to occur inbetween.
    pub fn begin_frame(&mut self) {
        // Submit the recorded cbuf to perform the draw calls
        self.cbuf_submit(
            // submit the cbuf for the current image
            self.cbufs[self.current_image as usize],
            self.present_queue,
            // wait_stages
            &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT],
            &[self.present_sema], // wait_semas
            &[self.render_sema], // signal_semas
        );
    }

    // Returns true if we are ready to call present
    pub fn frame_submission_complete(&mut self) -> bool {
        match unsafe { self.dev.get_fence_status(self.submit_fence) } {
            // true means vk::Result::SUCCESS
            // false means vk::Result::NOT_READY
            Ok(complete) => return complete,
            Err(_) => panic!("Failed to get fence status"),
        };
        
    }

    // Present the current swapchain image to the screen
    //
    // Finally we can actually flip the buffers and present
    // this image. 
    pub fn present(&mut self) {
        unsafe {
            self.dev.wait_for_fences(&[self.submit_fence],
                                     true, // wait for all
                                     std::u64::MAX, //timeout
            ).unwrap();
        }

        let wait_semas = [self.render_sema];
        let swapchains = [self.swapchain];
        let indices = [self.current_image];
        let info = vk::PresentInfoKHR::builder()
            .wait_semaphores(&wait_semas)
            .swapchains(&swapchains)
            .image_indices(&indices);

        unsafe {
            self.swapchain_loader
                .queue_present(self.present_queue, &info)
                .unwrap();
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
            log!(LogLevel::profiling, "Stoping the renderer");

            // first wait for the device to finish working
            self.dev.device_wait_idle().unwrap();

            // first destroy the application specific resources
            if let Some(ctx) = &mut *self.app_ctx.borrow_mut() {

                self.free_memory(ctx.vert_buffer_memory);
                self.free_memory(ctx.index_buffer_memory);
                self.dev.destroy_buffer(ctx.vert_buffer, None);
                self.dev.destroy_buffer(ctx.index_buffer, None);

                self.dev.destroy_sampler(ctx.image_sampler, None);

                self.dev.destroy_buffer(ctx.uniform_buffer, None);
                self.free_memory(ctx.uniform_buffers_memory);

                self.dev.destroy_render_pass(ctx.pass, None);

                self.dev.destroy_descriptor_set_layout(
                    ctx.descriptor_uniform_layout, None
                );

                self.dev.destroy_descriptor_pool(ctx.uniform_pool, None);
                ctx.desc_pool.destroy(&self);

                self.dev.destroy_pipeline_layout(ctx.pipeline_layout, None);

                for m in ctx.shader_modules.iter() {
                    self.dev.destroy_shader_module(*m, None);
                }

                for f in ctx.framebuffers.iter() {
                    self.dev.destroy_framebuffer(*f, None);
                }

                self.dev.destroy_pipeline(ctx.pipeline, None);
            }

            self.dev.destroy_semaphore(self.present_sema, None);
            self.dev.destroy_semaphore(self.render_sema, None);

            self.free_memory(self.depth_image_mem);
            self.dev.destroy_image_view(self.depth_image_view, None);
            self.dev.destroy_image(self.depth_image, None);
            
            for &view in self.views.iter() {
                self.dev.destroy_image_view(view, None);
            }

            self.dev.destroy_command_pool(self.pool, None);

            self.swapchain_loader.destroy_swapchain(self.swapchain, None);
            self.dev.destroy_fence(self.submit_fence, None);
            self.dev.destroy_device(None);

            self.display.destroy();

            self.debug_loader
                .destroy_debug_report_callback(self.debug_callback, None);
            self.inst.destroy_instance(None);
        }
    }
}
