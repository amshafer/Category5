/*
 * Playing around with some vulkan code
 *
 */

#![allow(non_camel_case_types)]
#[macro_use]
extern crate ash;
extern crate winit;

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::cell::RefCell;

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

pub use ash::version::{DeviceV1_0, EntryV1_0, InstanceV1_0};
use ash::{vk, Device, Entry, Instance};
use ash::extensions::{
    ext::DebugReport,
    khr::{Surface, Swapchain},
};

#[cfg(target_os = "macos")]
unsafe fn create_surface<E: EntryV1_0, I: InstanceV1_0>(
    entry: &E,
    instance: &I,
    window: &winit::Window,
) -> Result<vk::SurfaceKHR, vk::Result> {
    use std::ptr;
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
    println!("{:?}", CStr::from_ptr(p_message));
    vk::FALSE
}

pub struct Renderer {
    pub window: winit::Window,
    pub event_loop: RefCell<winit::EventsLoop>,
    pub debug_loader: DebugReport,
    pub debug_callback: vk::DebugReportCallbackEXT,

    pub loader: Entry,
    pub inst: Instance,
    pub dev: Device,
    pub pdev: vk::PhysicalDevice,
    pub queue_idx: u32,
    pub present_queue: vk::Queue,

    pub surface_loader: Surface,
    pub surface: vk::SurfaceKHR,
    pub swapchain_loader: Swapchain,
    pub swapchain: vk::SwapchainKHR,
    
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub depth_image: vk::Image,
    pub depth_image_view: vk::ImageView,
    pub depth_image_mem: vk::DeviceMemory,

    pub pool: vk::CommandPool,
    pub cbuffs: Vec<vk::CommandBuffer>,

    pub present_sema: vk::Semaphore,
    pub render_sema: vk::Semaphore,
}

impl Renderer {

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

    pub unsafe fn is_valid_queue_family(pdevice: vk::PhysicalDevice,
                                 info: vk::QueueFamilyProperties,
                                 index: u32,
                                 surface_loader: &Surface,
                                 surface: vk::SurfaceKHR) -> bool {
        info.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            && surface_loader
        // we need to make sure that this queue can actually
        // present the surface we have created
            .get_physical_device_surface_support(
                pdevice,
                index,
                surface,
            )
    }

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

    pub unsafe fn get_pdev_mem_properties(inst: &Instance,
                                          pdev: vk::PhysicalDevice)
                                          -> vk::PhysicalDeviceMemoryProperties
    {
        inst.get_physical_device_memory_properties(pdev)
    }

    pub unsafe fn create_surface
        (entry: &Entry, inst: &Instance, window: &winit::Window)
         -> vk::SurfaceKHR
    {
        create_surface(entry, inst, window).unwrap()
    }

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

    // create a logical device from a physical device
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

    pub unsafe fn create_swapchain(swapchain_loader: &Swapchain,
                                   surface_loader: &Surface,
                                   pdev: vk::PhysicalDevice,
                                   surface: vk::SurfaceKHR,
                                   surface_caps: &vk::SurfaceCapabilitiesKHR,
                                   surface_format: vk::SurfaceFormatKHR,
                                   resolution: &vk::Extent2D)
                                   -> vk::SwapchainKHR
    {
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
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(mode)
            .clipped(true)
            .image_array_layers(1);

        swapchain_loader
            .create_swapchain(&create_info, None)
            .unwrap()
    }

    pub unsafe fn create_command_pool(dev: &Device,
                                      queue_family: u32)
                                      -> vk::CommandPool
    {
        let pool_create_info = vk::CommandPoolCreateInfo::builder()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(queue_family);

        dev.create_command_pool(&pool_create_info, None).unwrap()
    }

    pub unsafe fn create_command_buffers(dev: &Device,
                                         pool: vk::CommandPool)
                                         -> Vec<vk::CommandBuffer>
    {
        let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(2)
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY);

        dev.allocate_command_buffers(&command_buffer_allocate_info)
            .unwrap()
    }

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

    pub unsafe fn create_image(dev: &Device,
                               mem_props: &vk::PhysicalDeviceMemoryProperties,
                               resolution: &vk::Extent2D,
                               usage: vk::ImageUsageFlags,
                               flags: vk::MemoryPropertyFlags)
                               -> (vk::Image, vk::ImageView, vk::DeviceMemory)
    {
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
                    .aspect_mask(vk::ImageAspectFlags::DEPTH)
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
                                       vk::MemoryPropertyFlags::DEVICE_LOCAL);

            let sema_create_info = vk::SemaphoreCreateInfo::default();

            let present_sema = dev
                .create_semaphore(&sema_create_info, None)
                .unwrap();
            let render_sema = dev
                .create_semaphore(&sema_create_info, None)
                .unwrap();

            Renderer {
                window: window,
                event_loop: RefCell::new(event_loop),
                debug_loader: dr_loader,
                debug_callback: d_callback,
                loader: entry,
                inst: inst,
                dev: dev,
                pdev: pdev,
                queue_idx: queue_family,
                present_queue: present_queue,
                surface_loader: surface_loader,
                surface: surface,
                swapchain_loader: swapchain_loader,
                swapchain: swapchain,
                images: images,
                views: image_views,
                depth_image: depth_image,
                depth_image_view: depth_image_view,
                depth_image_mem: depth_image_mem,
                pool: pool,
                cbuffs: buffers,
                present_sema: present_sema,
                render_sema: render_sema,
            }
        }
    }

    // records and submits a one-time command buffer
    // all operations taking place in 
    pub unsafe fn cbuff_onetime<F: FnOnce(&Renderer, vk::CommandBuffer)>
        (&mut self, record_func: F)
    {
        
    }

    pub fn setup_depth_image(&mut self) {
        unsafe {
            
        }
    }
    
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            println!("Stoping the renderer");
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

fn main() {
    Renderer::new();
    println!("Hello, world!");
}
