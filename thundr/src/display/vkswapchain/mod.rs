/// Display backends that use VkSwapchain
///
/// These backends have the per-frame images controlled by
/// Vulkan.
///
/// Austin Shafer - 2024
#[cfg(feature = "sdl")]
mod sdl;
mod vkd2d;

use ash::extensions::khr;
use ash::vk;
use ash::Entry;

use super::{DisplayInfoPayload, DisplayState, Swapchain};
use crate::device::Device;
use crate::{CreateInfo, Result as ThundrResult, SurfaceType, ThundrError, WindowInfo};
use utils::log;

use std::str::FromStr;
use std::sync::Arc;

/// This is our output info payload that Dakota will use to
/// initialize a new swapchain.
#[derive(Clone)]
pub(crate) struct VkSwapchainPayload {
    // function pointer loaders
    pub sp_surface_loader: khr::Surface,
}

impl DisplayInfoPayload for VkSwapchainPayload {
    fn max_output_count(&self) -> usize {
        usize::MAX
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// VkSwapchainKHR based outputs
///
/// These outputs use the Vulkan Swapchain and VkSurfaceKHR extensions to
/// allow the vulkan driver to handle the swapchain ordering. The major
/// two implementations are using SDL and using Direct to Display.
pub(crate) struct VkSwapchain {
    d_dev: Arc<Device>,
    /// The OutputInfo this swapchain was created from
    d_payload: Arc<dyn DisplayInfoPayload>,
    // the actual surface (KHR extension)
    pub d_surface: vk::SurfaceKHR,
    d_back: Box<dyn VkSwapchainBackend>,
    /// Cache the present mode here so we don't re-request it
    pub d_present_mode: vk::PresentModeKHR,

    /// loads swapchain extension
    pub(crate) d_swapchain_loader: khr::Swapchain,
    /// the actual swapchain
    pub(crate) d_swapchain: vk::SwapchainKHR,
}

pub(crate) trait VkSwapchainBackend {
    /// Get an x11 display surface.
    fn create_surface(
        &self,
        entry: &Entry,        // entry and inst aren't used but still need
        inst: &ash::Instance, // to be passed for compatibility
        pdev: vk::PhysicalDevice,
        surface_loader: &khr::Surface,
        win_info: &WindowInfo,
    ) -> Result<vk::SurfaceKHR, vk::Result>;

    /// Get the dots per inch for this surface
    ///
    /// This is useful because some upper parts of the stack (dakota
    /// text management) need to know this value to calculate the
    /// resolution of things.
    fn get_dpi(&self) -> ThundrResult<(i32, i32)>;

    /// Helper for getting the drawable size according to the
    /// display. This will basically just be passed to SDL's
    /// function of the same name.
    /// Returns None if not supported and the display should
    /// get the size from vulkan
    fn get_vulkan_drawable_size(&self) -> Option<vk::Extent2D>;
}

impl VkSwapchain {
    /// Check if a queue family is suited for our needs.
    /// Queue families need to support graphical presentation and
    /// presentation on the given surface.
    fn is_valid_queue_family(&self, info: vk::QueueFamilyProperties, index: u32) -> bool {
        let payload = self
            .d_payload
            .as_any()
            .downcast_ref::<VkSwapchainPayload>()
            .unwrap();

        info.queue_flags.contains(vk::QueueFlags::GRAPHICS)
            && unsafe {
                payload
                    .sp_surface_loader
                    // ensure compatibility with the surface
                    .get_physical_device_surface_support(self.d_dev.pdev, index, self.d_surface)
                    .unwrap()
            }
    }

    /// choose a vkSurfaceFormatKHR for the vkSurfaceKHR
    ///
    /// This selects the color space and layout for a surface. This should
    /// be called by the Renderer after creating a Display.
    fn select_surface_format(&self) -> ThundrResult<vk::SurfaceFormatKHR> {
        let payload = self
            .d_payload
            .as_any()
            .downcast_ref::<VkSwapchainPayload>()
            .unwrap();

        let formats = unsafe {
            payload
                .sp_surface_loader
                .get_physical_device_surface_formats(self.d_dev.pdev, self.d_surface)
                .or(Err(ThundrError::INVALID))?
        };

        // TODO: For now force the use of a B8G8R8A8_UNORM. Without doing this we end up
        // with mismatching colors because we assume UNORM everywhere
        formats
            .iter()
            .find(|fmt| fmt.format == vk::Format::B8G8R8A8_UNORM)
            .ok_or(ThundrError::INVALID_FORMAT)
            .copied()
    }

    /// Get the vkImage's for the swapchain, and create vkImageViews for them
    ///
    /// get all the presentation images for the swapchain
    /// specify the image views, which specify how we want
    /// to access our images
    fn select_images_and_views(&mut self, dstate: &mut DisplayState) -> ThundrResult<()> {
        let images = unsafe {
            self.d_swapchain_loader
                .get_swapchain_images(self.d_swapchain)
                .or(Err(ThundrError::COULD_NOT_CREATE_IMAGE))?
        };

        let mut image_views = Vec::new();
        for image in images.iter() {
            let _format_props = unsafe {
                self.d_dev.inst.inst.get_physical_device_format_properties(
                    self.d_dev.pdev,
                    dstate.d_surface_format.format,
                )
            };
            log::info!("format props: {:#?}", _format_props);

            // we want to interact with this image as a 2D
            // array of RGBA pixels (i.e. the "normal" way)
            let create_info = vk::ImageViewCreateInfo::builder()
                .view_type(vk::ImageViewType::TYPE_2D)
                // see `create_swapchain` for why we don't use surface_format
                .format(dstate.d_surface_format.format)
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

            unsafe {
                image_views.push(
                    self.d_dev
                        .dev
                        .create_image_view(&create_info, None)
                        .or(Err(ThundrError::COULD_NOT_CREATE_IMAGE))?,
                );
            }
        }

        dstate.d_images = images;
        dstate.d_views = image_views;

        Ok(())
    }

    /// Tear down all the swapchain-dependent vulkan objects we have created.
    /// This will be used when dropping everything and when we need to handle
    /// OOD events.
    fn destroy_swapchain(&mut self) {
        unsafe {
            self.d_swapchain_loader
                .destroy_swapchain(self.d_swapchain, None);
            self.d_swapchain = vk::SwapchainKHR::null();
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
    fn create_swapchain(&mut self, dstate: &mut DisplayState) -> ThundrResult<()> {
        // how many images we want the swapchain to contain
        // Default to double buffering for minimal input lag.
        let mut desired_image_count = 2;
        if desired_image_count < dstate.d_surface_caps.min_image_count {
            desired_image_count = dstate.d_surface_caps.min_image_count;
        }

        let transform = if dstate
            .d_surface_caps
            .supported_transforms
            .contains(vk::SurfaceTransformFlagsKHR::IDENTITY)
        {
            vk::SurfaceTransformFlagsKHR::IDENTITY
        } else {
            dstate.d_surface_caps.current_transform
        };

        let create_info = vk::SwapchainCreateInfoKHR::builder()
            .flags(vk::SwapchainCreateFlagsKHR::empty())
            .surface(self.d_surface)
            .min_image_count(desired_image_count)
            .image_color_space(dstate.d_surface_format.color_space)
            .image_format(dstate.d_surface_format.format)
            .image_extent(dstate.d_resolution)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(self.d_present_mode)
            .clipped(true)
            .image_array_layers(1)
            .old_swapchain(self.d_swapchain)
            .build();

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

    /// Fetch the drawable size from the Vulkan surface
    fn get_vulkan_drawable_size(&self) -> vk::Extent2D {
        let payload = self
            .d_payload
            .as_any()
            .downcast_ref::<VkSwapchainPayload>()
            .unwrap();

        match self.d_back.get_vulkan_drawable_size() {
            Some(size) => size,
            None => {
                // If the backend doesn't support this then just get the
                // value from vulkan
                unsafe {
                    payload
                        .sp_surface_loader
                        .get_physical_device_surface_capabilities(self.d_dev.pdev, self.d_surface)
                        .expect("Could not get physical device surface capabilities")
                        .current_extent
                }
            }
        }
    }

    /// Create a Display Info entry for this backend
    ///
    /// For now this just creates one. vkd2d will need more in the future.
    pub fn get_display_info_list(dev: &Device) -> ThundrResult<Vec<Arc<dyn DisplayInfoPayload>>> {
        Ok(vec![Arc::new(VkSwapchainPayload {
            sp_surface_loader: khr::Surface::new(&dev.inst.loader, &dev.inst.inst),
        })])
    }

    /// Choose a backend and create a new Vulkan based Swapchain
    pub fn new(info: &CreateInfo, dev: Arc<Device>) -> ThundrResult<Self> {
        unsafe {
            let entry = &dev.inst.loader;
            let inst = &dev.inst.inst;
            let payload = info
                .payload
                .as_ref()
                .unwrap()
                .as_any()
                .downcast_ref::<VkSwapchainPayload>()
                .unwrap();

            let (back, surf, _) = match &info.surface_type {
                SurfaceType::Display => vkd2d::PhysicalDisplay::new(
                    entry,
                    inst,
                    dev.pdev,
                    &payload.sp_surface_loader,
                    &info.window_info,
                ),
                #[cfg(feature = "sdl")]
                SurfaceType::SDL2 => sdl::SDL2DisplayBackend::new(
                    entry,
                    inst,
                    dev.pdev,
                    &payload.sp_surface_loader,
                    &info.window_info,
                ),
                _ => panic!("Unsupported surface type"),
            }
            .unwrap();

            // the best mode for presentation is FIFO (with triple buffering)
            // as this is recommended by the samsung developer page, which
            // I am *assuming* is a good reference for low power apps
            let present_modes = payload
                .sp_surface_loader
                .get_physical_device_surface_present_modes(dev.pdev, surf)
                .unwrap();
            let mode = present_modes
                .iter()
                .cloned()
                .find(|&mode| mode == vk::PresentModeKHR::MAILBOX)
                // fallback to FIFO if the mailbox mode is not available
                .unwrap_or(vk::PresentModeKHR::FIFO);

            let swapchain_loader = khr::Swapchain::new(&dev.inst.inst, &dev.dev);

            Ok(Self {
                d_dev: dev,
                d_payload: info.payload.clone().unwrap(),
                d_back: back,
                d_surface: surf,
                d_present_mode: mode,
                d_swapchain_loader: swapchain_loader,
                d_swapchain: vk::SwapchainKHR::null(),
            })
        }
    }
}

impl Swapchain for VkSwapchain {
    /// Choose a queue family
    ///
    /// returns an index into the array of queue types.
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface
    fn select_queue_family(&self) -> ThundrResult<u32> {
        let inst = &self.d_dev.inst.inst;

        // get the properties per queue family
        unsafe { inst.get_physical_device_queue_family_properties(self.d_dev.pdev) }
            // for each property info
            .iter()
            .enumerate()
            .filter_map(|(index, info)| {
                // add the device and the family to a list of
                // candidates for use later
                match self.is_valid_queue_family(*info, index as u32) {
                    // return the pdevice/family pair
                    true => Some(index as u32),
                    false => None,
                }
            })
            .nth(0)
            .ok_or(ThundrError::VK_SURF_NOT_SUPPORTED)
    }

    /// Get the surface information
    ///
    /// These capabilities are used elsewhere to identify swapchain
    /// surface capabilities. Even if the swapchain doesn't actually
    /// use VkSurfaceKHR these will still be filled in.
    fn get_surface_info(&self) -> ThundrResult<(vk::SurfaceCapabilitiesKHR, vk::SurfaceFormatKHR)> {
        let payload = self
            .d_payload
            .as_any()
            .downcast_ref::<VkSwapchainPayload>()
            .unwrap();

        let surface_caps = unsafe {
            payload
                .sp_surface_loader
                .get_physical_device_surface_capabilities(self.d_dev.pdev, self.d_surface)
                .unwrap()
        };
        let surface_format = self.select_surface_format().unwrap();

        Ok((surface_caps, surface_format))
    }

    /// Recreate our swapchain.
    ///
    /// This will be done on VK_ERROR_OUT_OF_DATE_KHR, signifying that
    /// the window is being resized and we have to regenerate accordingly.
    /// Keep in mind the Pipeline in Thundr will also have to be recreated
    /// separately.
    fn recreate_swapchain(&mut self, dstate: &mut DisplayState) -> ThundrResult<()> {
        // first wait for the device to finish working
        unsafe { self.d_dev.dev.device_wait_idle().unwrap() };

        // We need to get the updated size of our swapchain. This
        // will be the current size of the surface in use. We should
        // also update Display.d_resolution while we are at it.
        let new_res = self.get_vulkan_drawable_size();
        // TODO: clamp resolution here
        dstate.d_resolution = new_res;

        self.create_swapchain(dstate)?;

        self.select_images_and_views(dstate)?;

        Ok(())
    }

    /// Get the Dots Per Inch for this display.
    ///
    /// For VK_KHR_display we will calculate it ourselves, and for
    /// SDL we will ask SDL to tell us it.
    fn get_dpi(&self) -> ThundrResult<(i32, i32)> {
        // Check for a user set DPI
        if let Ok(env) = std::env::var("THUNDR_DPI") {
            let val: i32 = i32::from_str(env.as_str())
                .expect("THUNDR_DPI value must be a valid 32-bit floating point number");
            log::debug!("Using user specified DPI {:?}", val);
            return Ok((val, val));
        }

        self.d_back.get_dpi()
    }

    /// Update self.current_image with the swapchain image to render to
    ///
    /// If the next image is not ready (i.e. if Vulkan returned NOT_READY or
    /// TIMEOUT), then this will loop on calling `vkAcquireNextImageKHR` until
    /// it gets a valid image. This has to be done on AMD hw or else the TIMEOUT
    /// error will get passed up the callstack and fail.
    fn get_next_swapchain_image(&mut self, dstate: &mut DisplayState) -> ThundrResult<()> {
        let present_sema = dstate.d_available_present_semas.pop().unwrap();

        loop {
            let ret = match unsafe {
                self.d_swapchain_loader.acquire_next_image(
                    self.d_swapchain,
                    0,            // use a zero timeout to immediately get the state
                    present_sema, // signals presentation
                    vk::Fence::null(),
                )
            } {
                // On success, put this sema in the in-use slot for this image
                Ok((index, _)) => {
                    log::debug!(
                        "Getting next swapchain image: Current {:?}, New {:?}",
                        dstate.d_current_image,
                        index
                    );

                    dstate.d_current_image = index;

                    // Recycle the old sema and put it on the available list
                    if let Some(sema) = dstate.d_present_semas[index as usize].take() {
                        dstate.d_available_present_semas.push(sema);
                    }
                    dstate.d_present_semas[index as usize] = Some(present_sema);

                    Ok(())
                }
                Err(vk::Result::NOT_READY) => {
                    log::debug!(
                        "vkAcquireNextImageKHR: vk::Result::NOT_READY: Current {:?}",
                        dstate.d_current_image
                    );
                    continue;
                }
                Err(vk::Result::TIMEOUT) => {
                    log::debug!(
                        "vkAcquireNextImageKHR: vk::Result::TIMEOUT: Current {:?}",
                        dstate.d_current_image
                    );
                    continue;
                }
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(ThundrError::OUT_OF_DATE),
                Err(vk::Result::SUBOPTIMAL_KHR) => Err(ThundrError::OUT_OF_DATE),
                // the call did not succeed
                Err(_) => Err(ThundrError::COULD_NOT_ACQUIRE_NEXT_IMAGE),
            };

            // If an error was returned, put the sema back on the list
            if ret.is_err() {
                dstate.d_available_present_semas.push(present_sema);
            }

            return ret;
        }
    }

    /// Present the current swapchain image to the screen.
    ///
    /// Finally we can actually flip the buffers and present
    /// this image.
    fn present(&mut self, dstate: &DisplayState) -> ThundrResult<()> {
        // We can't wait for a timeline semaphore here, so instead wait for a semaphore
        // we signal during the last cbuf submitted in a frame
        let wait_semas = &[dstate.d_frame_sema];
        let swapchains = [self.d_swapchain];
        let indices = [dstate.d_current_image];
        let info = vk::PresentInfoKHR::builder()
            .wait_semaphores(wait_semas)
            .swapchains(&swapchains)
            .image_indices(&indices);

        unsafe {
            match self
                .d_swapchain_loader
                .queue_present(dstate.d_present_queue, &info)
            {
                Ok(_) => Ok(()),
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => Err(ThundrError::OUT_OF_DATE),
                Err(vk::Result::SUBOPTIMAL_KHR) => Err(ThundrError::OUT_OF_DATE),
                Err(_) => Err(ThundrError::PRESENT_FAILED),
            }
        }
    }
}

impl Drop for VkSwapchain {
    fn drop(&mut self) {
        println!("Destroying swapchain");
        unsafe {
            self.d_dev.dev.device_wait_idle().unwrap();
            self.destroy_swapchain();

            let payload = self
                .d_payload
                .as_any()
                .downcast_ref::<VkSwapchainPayload>()
                .unwrap();
            payload
                .sp_surface_loader
                .destroy_surface(self.d_surface, None);
        }
    }
}
