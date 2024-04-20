// The Display object owned by Renderer
//
// Austin Shafer - 2020

extern crate ash;

#[cfg(feature = "sdl")]
mod sdl;
mod vkd2d;

use ash::extensions::khr;
use ash::vk;
use ash::Entry;

use crate::device::Device;
use crate::{CreateInfo, Result as ThundrResult, SurfaceType, ThundrError};
use utils::log;

use std::str::FromStr;
use std::sync::Arc;

/// Shared state that subsystems consume. We need this
/// since Display holds rendering objects, but also has
/// to pass down swapchain/image info so those rendering
/// objects can access them
pub struct DisplayState {
    /// views describing how to access the images
    ///
    /// The Renderer's Pipeline depends on these, so
    /// when they are changed those resources will also
    /// have to be generated.
    pub(crate) d_views: Vec<vk::ImageView>,
    /// Current resolution of this output
    pub d_resolution: vk::Extent2D,
    // Vulkan surface capabilities
    pub d_surface_caps: vk::SurfaceCapabilitiesKHR,
    pub d_surface_format: vk::SurfaceFormatKHR,
    /// index into swapchain images that we are currently using
    pub(crate) d_current_image: u32,
    /// This signals that the latest contents have been presented.
    /// It is signaled by acquire next image and is consumed by
    /// the cbuf submission
    pub(crate) d_present_sema: vk::Semaphore,
    /// processes things to be physically displayed
    pub(crate) d_present_queue: vk::Queue,
    /// Frame end semaphore
    pub(crate) d_frame_sema: vk::Semaphore,
}

/// A display represents a physical screen
///
/// This is mostly the same as vulkan's concept of a display,
/// but it is a bit different. This name is overloaded as vulkan,
/// ash, and us have something called a display. Essentially
/// this holds the PFN loaders, the display KHR extension object,
/// and the surface generated for the physical display.
///
/// The swapchain is generated (and regenerated) from this stuff.
pub struct Display {
    d_dev: Arc<Device>,
    /// State to share with Renderer
    pub(crate) d_state: DisplayState,
    // the actual surface (KHR extension)
    pub d_surface: vk::SurfaceKHR,
    // function pointer loaders
    pub d_surface_loader: khr::Surface,
    d_back: Box<dyn Backend>,
    /// Cache the present mode here so we don't re-request it
    pub d_present_mode: vk::PresentModeKHR,

    /// loads swapchain extension
    pub(crate) d_swapchain_loader: khr::Swapchain,
    /// the actual swapchain
    pub(crate) d_swapchain: vk::SwapchainKHR,

    /// a set of images belonging to swapchain
    pub(crate) d_images: Vec<vk::Image>,
}

pub(crate) trait Backend {
    /// Get an x11 display surface.
    fn create_surface(
        &self,
        entry: &Entry,        // entry and inst aren't used but still need
        inst: &ash::Instance, // to be passed for compatibility
        pdev: vk::PhysicalDevice,
        surface_loader: &khr::Surface,
        surf_type: &SurfaceType,
    ) -> Result<vk::SurfaceKHR, vk::Result>;

    /// Get the dots per inch for this surface
    ///
    /// This is useful because some upper parts of the stack (dakota
    /// text management) need to know this value to calculate the
    /// resolution of things.
    fn get_dpi(&self) -> ThundrResult<(f32, f32)>;

    /// Helper for getting the drawable size according to the
    /// display. This will basically just be passed to SDL's
    /// function of the same name.
    /// Returns None if not supported and the display should
    /// get the size from vulkan
    fn get_vulkan_drawable_size(&self) -> Option<vk::Extent2D>;
}

enum BackendType {
    PhysicalDisplay,
    #[cfg(feature = "sdl")]
    SDL2Display,
}

impl Display {
    fn choose_display_backend(info: &CreateInfo) -> BackendType {
        match info.surface_type {
            SurfaceType::Display(_) => BackendType::PhysicalDisplay,
            #[cfg(feature = "sdl")]
            SurfaceType::SDL2(_, _) => BackendType::SDL2Display,
        }
    }

    /// Check if a queue family is suited for our needs.
    /// Queue families need to support graphical presentation and
    /// presentation on the given surface.
    fn is_valid_queue_family(
        pdevice: vk::PhysicalDevice,
        info: vk::QueueFamilyProperties,
        index: u32,
        surface_loader: &khr::Surface,
        surface: vk::SurfaceKHR,
        flags: vk::QueueFlags,
    ) -> bool {
        info.queue_flags.contains(flags)
            && unsafe {
                surface_loader
                    // ensure compatibility with the surface
                    .get_physical_device_surface_support(pdevice, index, surface)
                    .unwrap()
            }
    }

    /// Choose a queue family
    ///
    /// returns an index into the array of queue types.
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface
    pub(crate) fn select_queue_family(
        inst: &ash::Instance,
        pdev: vk::PhysicalDevice,
        surface_loader: &khr::Surface,
        surface: vk::SurfaceKHR,
        flags: vk::QueueFlags,
    ) -> ThundrResult<u32> {
        // get the properties per queue family
        unsafe { inst.get_physical_device_queue_family_properties(pdev) }
            // for each property info
            .iter()
            .enumerate()
            .filter_map(|(index, info)| {
                // add the device and the family to a list of
                // candidates for use later
                match Self::is_valid_queue_family(
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
            .ok_or(ThundrError::VK_SURF_NOT_SUPPORTED)
    }

    pub fn new(info: &CreateInfo, dev: Arc<Device>) -> ThundrResult<Display> {
        unsafe {
            let entry = &dev.inst.loader;
            let inst = &dev.inst.inst;

            let s_loader = khr::Surface::new(entry, inst);
            let (back, surf, res) = match &info.surface_type {
                SurfaceType::Display(_) => vkd2d::PhysicalDisplay::new(
                    entry,
                    inst,
                    dev.pdev,
                    &s_loader,
                    &info.surface_type,
                ),
                #[cfg(feature = "sdl")]
                SurfaceType::SDL2(_, _) => sdl::SDL2DisplayBackend::new(
                    entry,
                    inst,
                    dev.pdev,
                    &s_loader,
                    &info.surface_type,
                ),
            }
            .unwrap();

            // the best mode for presentation is FIFO (with triple buffering)
            // as this is recommended by the samsung developer page, which
            // I am *assuming* is a good reference for low power apps
            let present_modes = s_loader
                .get_physical_device_surface_present_modes(dev.pdev, surf)
                .unwrap();
            let mode = present_modes
                .iter()
                .cloned()
                .find(|&mode| mode == vk::PresentModeKHR::FIFO)
                // fallback to FIFO if the mailbox mode is not available
                .unwrap_or(vk::PresentModeKHR::FIFO);
            let surface_caps = s_loader
                .get_physical_device_surface_capabilities(dev.pdev, surf)
                .unwrap();
            let surface_format = Self::select_surface_format(&s_loader, surf, dev.pdev).unwrap();

            // Ensure that there is a valid queue, validation layer checks for this
            let graphics_queue_family = Self::select_queue_family(
                &dev.inst.inst,
                dev.pdev,
                &s_loader,
                surf,
                vk::QueueFlags::GRAPHICS,
            )?;
            let present_queue = dev.dev.get_device_queue(graphics_queue_family, 0);

            let swapchain_loader = khr::Swapchain::new(&dev.inst.inst, &dev.dev);
            let sema_create_info = vk::SemaphoreCreateInfo::default();
            let present_sema = dev.dev.create_semaphore(&sema_create_info, None).unwrap();
            let frame_sema = dev.dev.create_semaphore(&sema_create_info, None).unwrap();

            let mut ret = Self {
                d_dev: dev,
                d_state: DisplayState {
                    d_surface_caps: surface_caps,
                    d_surface_format: surface_format,
                    d_resolution: res,
                    d_views: Vec::with_capacity(0),
                    d_current_image: 0,
                    d_present_sema: present_sema,
                    d_present_queue: present_queue,
                    d_frame_sema: frame_sema,
                },
                d_surface_loader: s_loader,
                d_back: back,
                d_surface: surf,
                d_present_mode: mode,
                d_swapchain_loader: swapchain_loader,
                d_swapchain: vk::SwapchainKHR::null(),
                d_images: Vec::with_capacity(0),
            };

            ret.recreate_swapchain()?;

            return Ok(ret);
        }
    }

    /// Populates this display with a new vkSwapchain
    ///
    /// Swapchains contain images that can be used for WSI presentation
    /// They take a vkSurfaceKHR and provide a way to manage swapping
    /// effects such as double/triple buffering (mailbox mode). The created
    /// swapchain is dependent on the characteristics and format of the surface
    /// it is created for.
    /// The application resolution is set by this method.
    fn create_swapchain(&mut self) -> ThundrResult<()> {
        // how many images we want the swapchain to contain
        let mut desired_image_count = self.d_state.d_surface_caps.min_image_count + 1;
        if self.d_state.d_surface_caps.max_image_count > 0
            && desired_image_count > self.d_state.d_surface_caps.max_image_count
        {
            desired_image_count = self.d_state.d_surface_caps.max_image_count;
        }

        let transform = if self
            .d_state
            .d_surface_caps
            .supported_transforms
            .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
        {
            vk::SurfaceTransformFlagsKHR::IDENTITY
        } else {
            self.d_state.d_surface_caps.current_transform
        };

        // see this for how to get storage swapchain on intel:
        // https://github.com/doitsujin/dxvk/issues/504

        let create_info = vk::SwapchainCreateInfoKHR::builder()
            .flags(vk::SwapchainCreateFlagsKHR::empty())
            .surface(self.d_surface)
            .min_image_count(desired_image_count)
            .image_color_space(self.d_state.d_surface_format.color_space)
            .image_format(self.d_state.d_surface_format.format)
            .image_extent(self.d_state.d_resolution)
            // the color attachment is guaranteed to be available
            //
            // WEIRD: validation layers throw an issue with this on intel since it doesn't
            // support storage for the swapchain format.
            // You can ignore this:
            // https://www.reddit.com/r/vulkan/comments/ahtw8x/shouldnt_validation_layers_catch_the_wrong_format/
            //
            // Leave the STORAGE flag to be explicit that we need it
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(self.d_present_mode)
            .clipped(true)
            .image_array_layers(1)
            .old_swapchain(self.d_swapchain);

        // views for all of the swapchains images will be set up in
        // select_images_and_views
        let new_swapchain = unsafe {
            self.d_swapchain_loader
                .create_swapchain(&create_info, None)
                .or(Err(ThundrError::COULD_NOT_CREATE_SWAPCHAIN))?
        };

        // Now that we recreated the swapchain destroy the old one
        self.destroy_swapchain();
        self.d_swapchain = new_swapchain;

        Ok(())
    }

    /// Get the vkImage's for the swapchain, and create vkImageViews for them
    ///
    /// get all the presentation images for the swapchain
    /// specify the image views, which specify how we want
    /// to access our images
    fn select_images_and_views(&mut self) -> ThundrResult<()> {
        let images = unsafe {
            self.d_swapchain_loader
                .get_swapchain_images(self.d_swapchain)
                .or(Err(ThundrError::COULD_NOT_CREATE_IMAGE))?
        };

        let mut image_views = Vec::new();
        for image in images.iter() {
            let format_props = unsafe {
                self.d_dev.inst.inst.get_physical_device_format_properties(
                    self.d_dev.pdev,
                    self.d_state.d_surface_format.format,
                )
            };
            log::info!("format props: {:#?}", format_props);

            // we want to interact with this image as a 2D
            // array of RGBA pixels (i.e. the "normal" way)
            let mut create_info = vk::ImageViewCreateInfo::builder()
                .view_type(vk::ImageViewType::TYPE_2D)
                // see `create_swapchain` for why we don't use surface_format
                .format(self.d_state.d_surface_format.format)
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
                .image(*image)
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

            unsafe {
                image_views.push(
                    self.d_dev
                        .dev
                        .create_image_view(&create_info, None)
                        .or(Err(ThundrError::COULD_NOT_CREATE_IMAGE))?,
                );
            }
        }

        self.d_images = images;
        self.d_state.d_views = image_views;

        Ok(())
    }

    /// Tear down all the swapchain-dependent vulkan objects we have created.
    /// This will be used when dropping everything and when we need to handle
    /// OOD events.
    fn destroy_swapchain(&mut self) {
        unsafe {
            // Don't destroy the images here, the destroy swapchain call
            // will take care of them
            for view in self.d_state.d_views.iter() {
                self.d_dev.dev.destroy_image_view(*view, None);
            }
            self.d_state.d_views.clear();

            self.d_swapchain_loader
                .destroy_swapchain(self.d_swapchain, None);
            self.d_swapchain = vk::SwapchainKHR::null();
        }
    }

    /// Recreate our swapchain.
    ///
    /// This will be done on VK_ERROR_OUT_OF_DATE_KHR, signifying that
    /// the window is being resized and we have to regenerate accordingly.
    /// Keep in mind the Pipeline in Thundr will also have to be recreated
    /// separately.
    pub fn recreate_swapchain(&mut self) -> ThundrResult<()> {
        // first wait for the device to finish working
        unsafe { self.d_dev.dev.device_wait_idle().unwrap() };

        // We need to get the updated size of our swapchain. This
        // will be the current size of the surface in use. We should
        // also update Display.d_resolution while we are at it.
        let new_res = self.get_vulkan_drawable_size(self.d_dev.pdev);
        // TODO: clamp resolution here
        self.d_state.d_resolution = new_res;

        self.create_swapchain()?;

        self.select_images_and_views()?;

        Ok(())
    }

    /// Get the Dots Per Inch for this display.
    ///
    /// For VK_KHR_display we will calculate it ourselves, and for
    /// SDL we will ask SDL to tell us it.
    pub fn get_dpi(&self) -> ThundrResult<(f32, f32)> {
        // Check for a user set DPI
        if let Ok(env) = std::env::var("THUNDR_DPI") {
            let val: f32 = f32::from_str(env.as_str())
                .expect("THUNDR_DPI value must be a valid 32-bit floating point number");
            log::debug!("Using user specified DPI {:?}", val);
            return Ok((val, val));
        }

        self.d_back.get_dpi()
    }

    /// Selects a resolution for the renderer
    ///
    /// We saved the resolution of the display surface when we created
    /// it. If the surface capabilities doe not specify a requested
    /// extent, then we will return the screen's resolution.
    pub unsafe fn select_resolution(&self) -> vk::Extent2D {
        match self.d_state.d_surface_caps.current_extent.width {
            std::u32::MAX => self.d_state.d_resolution,
            _ => self.d_state.d_surface_caps.current_extent,
        }
    }

    /// choose a vkSurfaceFormatKHR for the vkSurfaceKHR
    ///
    /// This selects the color space and layout for a surface. This should
    /// be called by the Renderer after creating a Display.
    fn select_surface_format(
        surface_loader: &khr::Surface,
        surface: vk::SurfaceKHR,
        pdev: vk::PhysicalDevice,
    ) -> ThundrResult<vk::SurfaceFormatKHR> {
        let formats = unsafe {
            surface_loader
                .get_physical_device_surface_formats(pdev, surface)
                .or(Err(ThundrError::INVALID))?
        };

        formats
            .iter()
            .map(|fmt| match fmt.format {
                // if the surface does not specify a desired format
                // then we can choose our own
                vk::Format::UNDEFINED => vk::SurfaceFormatKHR {
                    format: vk::Format::B8G8R8A8_UNORM,
                    color_space: fmt.color_space,
                },
                // if the surface has a desired format we will just
                // use that
                _ => *fmt,
            })
            .nth(0)
            .ok_or(ThundrError::INVALID_FORMAT)
    }

    pub fn extension_names(info: &CreateInfo) -> Vec<*const i8> {
        match Self::choose_display_backend(info) {
            BackendType::PhysicalDisplay => {
                vkd2d::PhysicalDisplay::extension_names(&info.surface_type)
            }
            #[cfg(feature = "sdl")]
            BackendType::SDL2Display => {
                sdl::SDL2DisplayBackend::extension_names(&info.surface_type).unwrap()
            }
        }
    }

    pub fn get_vulkan_drawable_size(&self, pdev: vk::PhysicalDevice) -> vk::Extent2D {
        match self.d_back.get_vulkan_drawable_size() {
            Some(size) => size,
            None => {
                // If the backend doesn't support this then just get the
                // value from vulkan
                unsafe {
                    self.d_surface_loader
                        .get_physical_device_surface_capabilities(pdev, self.d_surface)
                        .expect("Could not get physical device surface capabilities")
                        .current_extent
                }
            }
        }
    }

    /// Update self.current_image with the swapchain image to render to
    ///
    /// If the next image is not ready (i.e. if Vulkan returned NOT_READY or
    /// TIMEOUT), then this will loop on calling `vkAcquireNextImageKHR` until
    /// it gets a valid image. This has to be done on AMD hw or else the TIMEOUT
    /// error will get passed up the callstack and fail.
    pub fn get_next_swapchain_image(&mut self) -> ThundrResult<()> {
        loop {
            match unsafe {
                self.d_swapchain_loader.acquire_next_image(
                    self.d_swapchain,
                    0,                           // use a zero timeout to immediately get the state
                    self.d_state.d_present_sema, // signals presentation
                    vk::Fence::null(),
                )
            } {
                // TODO: handle suboptimal surface regeneration
                Ok((index, _)) => {
                    log::debug!(
                        "Getting next swapchain image: Current {:?}, New {:?}",
                        self.d_state.d_current_image,
                        index
                    );
                    self.d_state.d_current_image = index;
                    return Ok(());
                }
                Err(vk::Result::NOT_READY) => {
                    log::debug!(
                        "vkAcquireNextImageKHR: vk::Result::NOT_READY: Current {:?}",
                        self.d_state.d_current_image
                    );
                    continue;
                }
                Err(vk::Result::TIMEOUT) => {
                    log::debug!(
                        "vkAcquireNextImageKHR: vk::Result::TIMEOUT: Current {:?}",
                        self.d_state.d_current_image
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

    /// Present the current swapchain image to the screen.
    ///
    /// Finally we can actually flip the buffers and present
    /// this image.
    pub fn present(&mut self) -> ThundrResult<()> {
        // We can't wait for a timeline semaphore here, so instead wait for a semaphore
        // we signal during the last cbuf submitted in a frame
        let wait_semas = &[self.d_state.d_frame_sema];
        let swapchains = [self.d_swapchain];
        let indices = [self.d_state.d_current_image];
        let info = vk::PresentInfoKHR::builder()
            .wait_semaphores(wait_semas)
            .swapchains(&swapchains)
            .image_indices(&indices);

        unsafe {
            match self
                .d_swapchain_loader
                .queue_present(self.d_state.d_present_queue, &info)
            {
                Ok(_) => Ok(()),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(ThundrError::OUT_OF_DATE),
                Err(vk::Result::SUBOPTIMAL_KHR) => Err(ThundrError::OUT_OF_DATE),
                Err(_) => Err(ThundrError::PRESENT_FAILED),
            }
        }
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        println!("Destroying display");
        unsafe {
            self.d_dev.dev.device_wait_idle().unwrap();
            self.d_dev
                .dev
                .destroy_semaphore(self.d_state.d_frame_sema, None);
            self.d_dev
                .dev
                .destroy_semaphore(self.d_state.d_present_sema, None);
            self.destroy_swapchain();
            self.d_surface_loader.destroy_surface(self.d_surface, None);
        }
    }
}
