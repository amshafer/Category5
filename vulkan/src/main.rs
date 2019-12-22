/*
 * Playing around with some vulkan code
 *
 * Very clearly derived from the ash examples
 */

#![allow(non_camel_case_types)]
#[macro_use]
extern crate ash;
extern crate winit;
use winit::*;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::cell::RefCell;
use std::io::Cursor;
use std::ptr;

pub use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0};
use ash::{vk, Device, Entry, Instance};
use ash::util;
use ash::extensions::{
    ext::DebugReport,
    khr::{Surface, Swapchain},
};

#[cfg(target_os = "macos")]
use std::mem;
#[cfg(target_os = "macos")]
use ash::extensions::mvk::MacOSSurface;
#[cfg(target_os = "macos")]
extern crate cocoa;
#[cfg(target_os = "macos")]
extern crate metal;
#[cfg(target_os = "macos")]
extern crate objc;
#[cfg(target_os = "macos")]
use cocoa::appkit::{NSView, NSWindow};
#[cfg(target_os = "macos")]
use cocoa::base::id as cocoa_id;
#[cfg(target_os = "macos")]
use metal::CoreAnimationLayer;
#[cfg(target_os = "macos")]
use objc::runtime::YES;

// MoltenVK provides vulkan on top of apple's metal api
// this function is straight from the ash examples
// It's included because i'm testing on my laptop
// until I start experimenting with X11-less BSD
#[cfg(target_os = "macos")]
unsafe fn create_surface<E: EntryV1_0, I: InstanceV1_0>(
    entry: &E,
    instance: &I,
    window: &winit::Window,
) -> Result<vk::SurfaceKHR, vk::Result> {
    use winit::os::macos::WindowExt;

    let wnd: cocoa_id = mem::transmute(window.get_nswindow());

    let layer = CoreAnimationLayer::new();

    layer.set_edge_antialiasing_mask(0);
    layer.set_presents_with_transaction(false);
    layer.remove_all_animations();

    let view = wnd.contentView();

    layer.set_contents_scale(view.backingScaleFactor());
    view.setLayer(mem::transmute(layer.as_ref()));
    view.setWantsLayer(YES);

    let create_info = vk::MacOSSurfaceCreateInfoMVK {
        s_type: vk::StructureType::MACOS_SURFACE_CREATE_INFO_M,
        p_next: ptr::null(),
        flags: Default::default(),
        p_view: window.get_nsview() as *const c_void,
    };

    let macos_surface_loader = MacOSSurface::new(entry, instance);
    macos_surface_loader.create_mac_os_surface_mvk(&create_info, None)
}

// this happy little debug callback is also from the ash examples
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
    println!("[RENDERER] {:?}", CStr::from_ptr(p_message));
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
    // these fields take care of windowing on the desktop
    // they will eventually be replaced
    pub window: winit::Window,
    pub event_loop: RefCell<winit::EventsLoop>,
    // debug callback sugar mentioned earlier
    pub debug_loader: DebugReport,
    pub debug_callback: vk::DebugReportCallbackEXT,

    // the entry just loads function pointers from the dynamic library
    // I am calling it a loader, because that's what it does
    pub loader: Entry,
    // the big vulkan instance.
    pub inst: Instance,
    // the logical device we are using
    // maybe I'll test around with multi-gpu
    pub dev: Device,
    // the physical device selected to display to
    pub pdev: vk::PhysicalDevice,

    // index into the array of queue families
    pub family_index: u32,
    // processes things to be physically displayed
    pub present_queue: vk::Queue,

    // loads surface extension functions
    pub surface_loader: Surface,
    // the actual surface (KHR extension)
    pub surface: vk::SurfaceKHR,
    pub surface_format: vk::SurfaceFormatKHR,
    // resolution we created the swapchain with
    pub resolution: vk::Extent2D,

    // loads swapchain extension
    pub swapchain_loader: Swapchain,
    // the actual swapchain
    pub swapchain: vk::SwapchainKHR,
    // index into swapchain images that we are currently using
    pub current_image: u32,

    // a set of images belonging to swapchain
    pub images: Vec<vk::Image>,
    // views describing how to access the images
    pub views: Vec<vk::ImageView>,

    // pools provide the memory allocated to command buffers
    pub pool: vk::CommandPool,
    // the command buffers allocated from pool
    pub cbufs: Vec<vk::CommandBuffer>,

    // ---- Application specific ----
    pub app_ctx: Option<AppContext>,

    // an image for recording depth test data
    pub depth_image: vk::Image,
    pub depth_image_view: vk::ImageView,
    // because we create the image, we need to back it with memory
    pub depth_image_mem: vk::DeviceMemory,

    // semaphores to tell us when presentation or rendering finished
    pub present_sema: vk::Semaphore,
    pub render_sema: vk::Semaphore,
}

pub struct AppContext {
    pub pass: vk::RenderPass,
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub shader_modules: Box<[vk::ShaderModule]>,
    pub framebuffers: Vec<vk::Framebuffer>,
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

    // creates a window in the current desktop environment
    // this will also one day be removed
    unsafe fn create_window() -> (winit::Window, winit::EventsLoop) {
        let events_loop = winit::EventsLoop::new();
        let window = winit::WindowBuilder::new()
            .with_title("Vulkan")
            .with_dimensions(winit::dpi::LogicalSize::new(
                f64::from(640),
                f64::from(480),
            ))
            .build(&events_loop)
            .unwrap();
        return (window, events_loop);
    }

    // Create a vkInstance
    //
    // Most of the create info entries are straightforward, with
    // the standard validation layers being enabled, along with some
    // basic extnesions.
    unsafe fn create_instance() -> (Entry, Instance) {
        let entry = Entry::new().unwrap();
        let app_name = CString::new("VulkanRenderer").unwrap();

        let layer_names = [CString::new("VK_LAYER_LUNARG_standard_validation")
                           .unwrap()];
        let layers_names_raw: Vec<*const i8> = layer_names
            .iter()
            .map(|raw_name| raw_name.as_ptr())
            .collect();

        let extension_names_raw = vec![
            Surface::name().as_ptr(),
            MacOSSurface::name().as_ptr(),
            DebugReport::name().as_ptr(),
        ];

        let appinfo = vk::ApplicationInfo::builder()
            .application_name(&app_name)
            .application_version(0)
            .engine_name(&app_name)
            .engine_version(0)
            .api_version(vk_make_version!(1, 1, 127));

        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(&appinfo)
            .enabled_layer_names(&layers_names_raw)
            .enabled_extension_names(&extension_names_raw);

        let instance: Instance = entry
            .create_instance(&create_info, None)
            .expect("Instance creation error");

        return (entry, instance);
    }

    // Check if a queue family is suited for our needs.
    // Queue families need to support graphical presentation and 
    // presentation on the given surface.
    pub unsafe fn is_valid_queue_family(pdevice: vk::PhysicalDevice,
                                 info: vk::QueueFamilyProperties,
                                 index: u32,
                                 surface_loader: &Surface,
                                 surface: vk::SurfaceKHR) -> bool {
        info.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            && surface_loader
            // ensure compatibility with the surface
            .get_physical_device_surface_support(
                pdevice,
                index,
                surface,
            )
    }

    // Choose a vkPhysicalDevice and queue family index
    //
    // selects a physical device and a queue family
    // provide the surface PFN loader and the surface so
    // that we can ensure the pdev/queue combination can
    // present the surface
    pub unsafe fn select_pdev_and_family(inst: &Instance,
                                         surface_loader: &Surface,
                                         surface: vk::SurfaceKHR)
                                         -> (vk::PhysicalDevice, u32)
    {
        let pdevices = inst
                .enumerate_physical_devices()
                .expect("Physical device error");

        // for each physical device
        pdevices
            .iter()
            .map(|pdevice| {
                // get the properties per queue family
                inst
                    .get_physical_device_queue_family_properties(*pdevice)
                    // for each property info
                    .iter()
                    .enumerate()
                    .filter_map(|(index, info)| {
                        // add the device and the family to a list of
                        // candidates for use later
                        if Renderer::is_valid_queue_family(*pdevice,
                                                           *info,
                                                           index as u32,
                                                           surface_loader,
                                                           surface) {
                            // return the pdevice/family pair
                            Some((*pdevice, index as u32))
                        } else {
                            None
                        }
                    })
                    .nth(0)
            })
            .filter_map(|v| v)
            .nth(0)
            // for now we are just going to get the first one
            .expect("Couldn't find suitable device.")
    }

    // get the vkPhysicalDeviceMemoryProperties structure for a vkPhysicalDevice
    pub unsafe fn get_pdev_mem_properties(inst: &Instance,
                                          pdev: vk::PhysicalDevice)
                                          -> vk::PhysicalDeviceMemoryProperties
    {
        inst.get_physical_device_memory_properties(pdev)
    }

    // for now just a wrapper to the global create surface
    pub unsafe fn create_surface
        (entry: &Entry, inst: &Instance, window: &winit::Window)
         -> vk::SurfaceKHR
    {
        create_surface(entry, inst, window).unwrap()
    }

    // choose a vkSurfaceFormatKHR for the vkSurfaceKHR
    //
    // This selects the color space and layout for a surface
    pub unsafe fn select_surface_format(pdev: vk::PhysicalDevice,
                                        loader: &Surface,
                                        surface: vk::SurfaceKHR)
                                        -> vk::SurfaceFormatKHR
    {
        let formats = loader.get_physical_device_surface_formats(pdev, surface)
            .unwrap();
        
        formats.iter()
            .map(|fmt| match fmt.format {
                // if the surface does not specify a desired format
                // then we can choose our own
                vk::Format::UNDEFINED => vk::SurfaceFormatKHR {
                    format: vk::Format::B8G8R8_UNORM,
                    color_space: fmt.color_space,
                },
                // if the surface has a desired format we will just
                // use that
                _ => *fmt,
            })
            .nth(0)
            .expect("Could not find a surface format")
    }

    // Selects a resolution for the renderer
    // for now this just selects the VGA's puny 640x480
    pub unsafe fn select_resolution(surface_caps: &vk::SurfaceCapabilitiesKHR)
                                    -> vk::Extent2D
    {
        match surface_caps.current_extent.width {
            std::u32::MAX => vk::Extent2D {
                // this should be a tunable at some point
                width: 640,
                height: 480,
            },
            _ => surface_caps.current_extent,
        }
    }

    // Create a vkDevice from a vkPhysicalDevice
    //
    // Create a logical device for interfacing with the physical device.
    // once again we specify any device extensions we need, the swapchain
    // being the most important one.
    //
    // A queue is created in the specified queue family in the
    // present_queue argument.
    pub unsafe fn create_device(inst: &Instance,
                                pdev: vk::PhysicalDevice,
                                present_queue: u32)
                                -> Device
    {
        let dev_extension_names = [Swapchain::name().as_ptr()];
        let features = vk::PhysicalDeviceFeatures {
            shader_clip_distance: 1,
            ..Default::default()
        };

        // for now we only have one queue, so one priority
        let priorities = [1.0];
        let queue_info = [vk::DeviceQueueCreateInfo::builder()
                          .queue_family_index(present_queue)
                          .queue_priorities(&priorities)
                          .build()];

        let dev_create_info = vk::DeviceCreateInfo::builder()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&dev_extension_names)
            .enabled_features(&features);

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
    pub unsafe fn create_swapchain(swapchain_loader: &Swapchain,
                                   surface_loader: &Surface,
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

        // the best mode for presentation is MAILBOX (triple buffering)
        let present_modes = surface_loader
            .get_physical_device_surface_present_modes(pdev, surface)
            .unwrap();
        let mode = present_modes
            .iter()
            .cloned()
            .find(|&mode| mode == vk::PresentModeKHR::MAILBOX)
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
    pub unsafe fn create_command_pool(dev: &Device,
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
    pub unsafe fn create_command_buffers(dev: &Device,
                                         pool: vk::CommandPool)
                                         -> Vec<vk::CommandBuffer>
    {
        let cbuf_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(2)
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
    pub unsafe fn select_images_and_views(swapchain_loader: &Swapchain,
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
    pub fn find_memory_type_index(props: &vk::PhysicalDeviceMemoryProperties,
                                  reqs: &vk::MemoryRequirements,
                                  flags: vk::MemoryPropertyFlags)
                                  -> Option<u32>
    {
        // for each memory type
        for (index, ref mem_type) in props.memory_types.iter().enumerate() {
            // vk::MemoryPropertyFlags::DEVICE_LOCAL is 1            
            if reqs.memory_type_bits & 1 == 1
                && mem_type.property_flags == flags {
                    println!("Selected type with flags {:?}",
                             mem_type.property_flags);
                    return Some(index as u32);
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
    pub unsafe fn create_image(dev: &Device,
                               mem_props: &vk::PhysicalDeviceMemoryProperties,
                               resolution: &vk::Extent2D,
                               usage: vk::ImageUsageFlags,
                               aspect: vk::ImageAspectFlags, 
                               flags: vk::MemoryPropertyFlags)
                               -> (vk::Image, vk::ImageView, vk::DeviceMemory)
    {
        // we create the image now, but will have to bind
        // some memory to it later.
        let create_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::D16_UNORM)
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

        // note the aspect here. This needs to be a parameter as
        // we will want to create multiple types in the future
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

    // Create a new Vulkan Renderer
    //
    // This renderer is very application specific. It is not meant to be
    // a generic safe wrapper for vulkan. This method constructs a new context,
    // creating a vulkan instance, finding a physical gpu, setting up a logical
    // device, and creating a swapchain.
    //
    // All methods called after this only need to take a mutable reference to
    // self, avoiding any nasty argument lists like the functions above. The goal
    // is to have this make dealing with the api less wordy.
    pub fn new() -> Renderer {
        unsafe {
            let (window, event_loop) = Renderer::create_window();

            let (entry, inst) = Renderer::create_instance();
            
            let (dr_loader, d_callback) = Renderer::setup_debug(&entry, &inst);

            let surface = Renderer::create_surface(&entry, &inst, &window);
            let surface_loader = Surface::new(&entry, &inst);

            let (pdev, queue_family) =
                Renderer::select_pdev_and_family(&inst,
                                                 &surface_loader,
                                                 surface);
            let mem_props = Renderer::get_pdev_mem_properties(&inst, pdev);

            let dev = Renderer::create_device(&inst, pdev, queue_family);
            let present_queue = dev.get_device_queue(queue_family, 0);

            // do this after we have gotten a valid physical device
            let surface_format = Renderer::select_surface_format(pdev,
                                                                 &surface_loader,
                                                                 surface);
            let surface_caps = surface_loader
                .get_physical_device_surface_capabilities(pdev, surface)
                .unwrap();

            let surface_resolution = Renderer::select_resolution(&surface_caps);

            let swapchain_loader = Swapchain::new(&inst, &dev);
            let swapchain = Renderer::create_swapchain(&swapchain_loader,
                                                       &surface_loader,
                                                       pdev,
                                                       surface,
                                                       &surface_caps,
                                                       surface_format,
                                                       &surface_resolution);

            let pool = Renderer::create_command_pool(&dev, queue_family);
            let buffers = Renderer::create_command_buffers(&dev, pool);
            
            let (images, image_views) =
                Renderer::select_images_and_views(&swapchain_loader,
                                                  swapchain,
                                                  &dev,
                                                  surface_format);

            // the depth attachment needs to have its own resources
            let (depth_image, depth_image_view, depth_image_mem) =
                Renderer::create_image(&dev,
                                       &mem_props,
                                       &surface_resolution,
                                       vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                                       vk::ImageAspectFlags::DEPTH,
                                       vk::MemoryPropertyFlags::DEVICE_LOCAL);

            let sema_create_info = vk::SemaphoreCreateInfo::default();

            let present_sema = dev
                .create_semaphore(&sema_create_info, None)
                .unwrap();
            let render_sema = dev
                .create_semaphore(&sema_create_info, None)
                .unwrap();

            // you are now the proud owner of a brand new rendering context
            Renderer {
                window: window,
                event_loop: RefCell::new(event_loop),
                debug_loader: dr_loader,
                debug_callback: d_callback,
                loader: entry,
                inst: inst,
                dev: dev,
                pdev: pdev,
                family_index: queue_family,
                present_queue: present_queue,
                surface_loader: surface_loader,
                surface: surface,
                surface_format: surface_format,
                swapchain_loader: swapchain_loader,
                swapchain: swapchain,
                current_image: 0,
                resolution: surface_resolution,
                images: images,
                views: image_views,
                depth_image: depth_image,
                depth_image_view: depth_image_view,
                depth_image_mem: depth_image_mem,
                pool: pool,
                cbufs: buffers,
                present_sema: present_sema,
                render_sema: render_sema,
                app_ctx: None,
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
    pub fn cbuf_onetime<F: FnOnce(&mut Renderer, vk::CommandBuffer)>
        (&mut self,
         cbuf: vk::CommandBuffer,
         queue: vk::Queue,
         wait_stages: &[vk::PipelineStageFlags],
         wait_semas: &[vk::Semaphore],
         signal_semas: &[vk::Semaphore],
         record_fn: F)
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
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);

            // start recording the command buffer, call the function
            // passed to load it with operations, and then end the
            // command buffer
            self.dev.begin_command_buffer(cbuf, &record_info)
                .expect("Could not start command buffer");

            record_fn(self, cbuf);

            self.dev.end_command_buffer(cbuf)
                .expect("Could not end command buffer");

            // once the one-time buffer has been recorded we can submit
            // it for execution.
            let submit_info = vk::SubmitInfo::builder()
                .wait_semaphores(wait_semas)
                .wait_dst_stage_mask(wait_stages)
                .command_buffers(&[cbuf])
                .signal_semaphores(signal_semas)
                .build();

            let fence = self.dev.create_fence(
                &vk::FenceCreateInfo::default(),
                None,
            ).expect("Could not create fence");

            // create a fence to be notified when the commands have finished
            // executing. Wait immediately for the fence.
            self.dev.queue_submit(queue, &[submit_info], fence)
                .expect("Could not submit buffer to queue");

            self.dev.wait_for_fences(&[fence],
                                     true, // wait for all
                                     std::u64::MAX, //timeout
            ).expect("Could not wait for the submit fence");
            // the commands are now executed
            self.dev.destroy_fence(fence, None);
        }
    }

    // set up the depth image in self.
    //
    // We need to transfer the format of the depth image to something
    // usable. We will use an image barrier to set the image as a depth
    // stencil attachment to be used later.
    pub unsafe fn setup_depth_image(&mut self) {
        // the depth image and view have already been created by new
        // we need to execute a cbuf to set up the memory we are
        // going to use later
        self.cbuf_onetime(
            self.cbufs[0], // use the first one for initialization
            self.present_queue,
            &[], // wait_stages
            &[], // wait_semas
            &[], // signal_semas
            // this closure will be the contents of the cbuf
            |rend, cbuf| {
                // We need to initialize the depth attachment by
                // performing a layout transition to the optimal
                // depth layout
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
    pub unsafe fn create_pass(&mut self) -> vk::RenderPass {
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
    pub unsafe fn create_shader_module(&mut self, cursor: &mut Cursor<&'static [u8]>)
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
    pub unsafe fn create_shader_stages(&mut self, entrypoint: *const i8)
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
    // This method roughly follows the "fixed function" part of the
    // vulkan tutorial.
    pub unsafe fn create_pipeline(&mut self,
                                  layout: vk::PipelineLayout,
                                  pass: vk::RenderPass,
                                  shader_stages: &[vk::PipelineShaderStageCreateInfo])
                                  -> vk::Pipeline
    {
        // now for the fixed function portions of the pipeline
        // for now vertices are specified in the shader
        let vertex_info = vk::PipelineVertexInputStateCreateInfo {
            vertex_attribute_description_count: 0,
            p_vertex_attribute_descriptions: ptr::null(),
            vertex_binding_description_count: 0,
            p_vertex_binding_descriptions: ptr::null(),
            ..Default::default()
        };

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
            front_face: vk::FrontFace::COUNTER_CLOCKWISE,
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
        // test. This should record Z-order to 1.0f
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
            src_color_blend_factor: vk::BlendFactor::SRC_ALPHA,
            dst_color_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            color_blend_op: vk::BlendOp::ADD,
            src_alpha_blend_factor: vk::BlendFactor::SRC_ALPHA,
            dst_alpha_blend_factor: vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
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
    pub unsafe fn create_framebuffers(&mut self,
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

    // Set up the application. This should *always* be called
    //
    // Once we have allocated a renderer with `new`, we should initialize
    // the rendering pipeline so that we can display things. This method
    // basically sets up all of the "application" specific resources like
    // shaders, geometry, and the like.
    pub fn setup(&mut self) {
        unsafe {
            self.setup_depth_image();
            
            let pass = self.create_pass();
            
            // This is a really annoying issue with CString ptrs
            let program_entrypoint_name = CString::new("main").unwrap();
            // If the CString is created in `create_shaders`, and is inserted in
            // the return struct using the `.as_ptr()` method, then the CString will
            // still be dropped on return and our pointer will be garbage. Instead
            // we need to ensure that the CString will live long enough. I have no
            // idea why it is like this.
            let shader_stages = Box::new(
                self.create_shader_stages(program_entrypoint_name.as_ptr())
            );

            // even though we don't have anything special in our layout, we
            // still need to have a created layout for the pipeline
            let layout_info = vk::PipelineLayoutCreateInfo::default();
            let layout = self.dev.create_pipeline_layout(&layout_info, None)
                .unwrap();
            
            let pipeline = self.create_pipeline(layout, pass, &*shader_stages);

            let framebuffers = self.create_framebuffers(pass, self.resolution);

            // The app context contains the scene specific data
            self.app_ctx = Some(AppContext {
                pass: pass,
                pipeline: pipeline,
                pipeline_layout: layout,
                framebuffers: framebuffers,
                shader_modules: shader_stages
                    .iter()
                    .map(|info| { info.module })
                    .collect(),
            });
        }
    }

    // Update self.current_image with the swapchain image to render to
    //
    // This index should be used by `start_frame`
    pub unsafe fn get_next_swapchain_image_index(&mut self) {
        let (idx, _) = self.swapchain_loader.acquire_next_image(
            self.swapchain,
            std::u64::MAX,
            self.present_sema,
            vk::Fence::null(),
        ).unwrap();

        self.current_image = idx;
    }

    // Render a frame, but do not present it
    //
    // Think of this as the "main" rendering operation. It will draw
    // all geometry to the current framebuffer. Presentation is
    // done later, in case operations need to occur inbetween.
    pub fn start_frame(&mut self) {
        unsafe {
            self.get_next_swapchain_image_index();

            // we need to clear both the color and depth attachments first
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
            if let Some(ctx) = &self.app_ctx {
                // We want to start a render pass to hold all of our drawing
                // The actual pass is started in the cbuf
                let pass_begin_info = vk::RenderPassBeginInfo::builder()
                    .render_pass(ctx.pass)
                    .framebuffer(ctx.framebuffers[self.current_image as usize])
                    .render_area(vk::Rect2D {
                        offset: vk::Offset2D { x: 0, y: 0 },
                        extent: self.resolution,
                    })
                    .clear_values(&clear_vals);

                // Create and submit a cbuf to perform the draw calls
                self.cbuf_onetime(
                    self.cbufs[1], // use the second one for drawing
                    self.present_queue,
                    // wait_stages
                    &[vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT],
                    &[self.present_sema], // wait_semas
                    &[self.render_sema], // signal_semas
                    // this closure will be the contents of the cbuf
                    |rend, cbuf| {
                        // begin a render pass. This is what drawing operations
                        // will be recorded in
                        rend.dev.cmd_begin_render_pass(
                            cbuf,
                            &pass_begin_info,
                            vk::SubpassContents::INLINE,
                        );

                        if let Some(app) = &rend.app_ctx {
                            rend.dev.cmd_bind_pipeline(
                                cbuf,
                                vk::PipelineBindPoint::GRAPHICS,
                                app.pipeline
                            );
                        }

                        // Here is where everything is actually drawn
                        // technically 3 vertices are being drawn by the shader
                        rend.dev.cmd_draw(
                            cbuf, // drawing command buffer
                            3, // number of verts
                            1, // number of instances
                            0, // first vertex
                            0, // first instance
                        );

                        // finish up our render pass
                        rend.dev.cmd_end_render_pass(cbuf);
                    },
                );
            }
        }
    }

    // Present the current swapchain image to the screen
    //
    // Finally we can actually flip the buffers and present
    // this image. 
    pub fn present(&mut self) {
        unsafe {
            let wait_semas = [self.render_sema];
            let swapchains = [self.swapchain];
            let indices = [self.current_image];
            let info = vk::PresentInfoKHR::builder()
                .wait_semaphores(&wait_semas)
                .swapchains(&swapchains)
                .image_indices(&indices);

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
            println!("Stoping the renderer");

            // first destroy the application specific resources
            if let Some(ctx) = &self.app_ctx {
                self.dev.device_wait_idle().unwrap();
                self.dev.destroy_render_pass(ctx.pass, None);
                self.dev.destroy_pipeline_layout(ctx.pipeline_layout, None);

                for m in ctx.shader_modules.iter() {
                    self.dev.destroy_shader_module(*m, None);
                }

                for f in ctx.framebuffers.iter() {
                    self.dev.destroy_framebuffer(*f, None);
                }

                self.dev.destroy_pipeline(ctx.pipeline, None);
            }
            
            // first wait for the device to finish working
            self.dev.device_wait_idle().unwrap();
            self.dev.destroy_semaphore(self.present_sema, None);
            self.dev.destroy_semaphore(self.render_sema, None);
            
            self.dev.free_memory(self.depth_image_mem, None);
            self.dev.destroy_image_view(self.depth_image_view, None);
            self.dev.destroy_image(self.depth_image, None);
            
            for &view in self.views.iter() {
                self.dev.destroy_image_view(view, None);
            }

            self.dev.destroy_command_pool(self.pool, None);

            self.swapchain_loader.destroy_swapchain(self.swapchain, None);
            self.dev.destroy_device(None);
            self.surface_loader.destroy_surface(self.surface, None);
            
            self.debug_loader
                .destroy_debug_report_callback(self.debug_callback, None);
            self.inst.destroy_instance(None);
        }
    }
}

// Try to keep this completely safe. Renderer should be usable
// from safe rust.
fn main() {
    // creates a context, swapchain, images, and others
    let mut rend = Renderer::new();
    // initialize the pipeline, renderpasses, and display engine
    rend.setup();

    println!("Begin render loop...");
    let mut cont = true;
    while cont {
        // draw a frame to be displayed
        rend.start_frame();
        // present our frame to the screen
        rend.present();

        // For winit to display anything we need to process the event loop
        // A window isn't created if this isn't here
        rend.event_loop.borrow_mut().poll_events(|event| {
            // window event nonsense from the example. This can be removed/modified
            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::KeyboardInput { input, .. } => {
                        if let Some(VirtualKeyCode::Escape) = input.virtual_keycode {
                            cont = false;
                        }
                    }
                    WindowEvent::CloseRequested => cont = false,
                    _ => (),
                },
                _ => (),
            }
        });
    }
}
