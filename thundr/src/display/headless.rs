/// Headless Display backend
///
/// Austin Shafer - 2024
use ash::vk;

use super::{DisplayInfoPayload, DisplayState, Swapchain};
use crate::device::Device;
use crate::{Result, ThundrError};

use std::sync::Arc;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;

/// Empty payload here since we have no state
struct HeadlessOutputPayload {}

impl DisplayInfoPayload for HeadlessOutputPayload {
    fn max_output_count(&self) -> usize {
        usize::MAX
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// A headless swapchain
///
/// For now this is simply used for testing. Defaults to
/// a 640x480 surface.
pub struct HeadlessSwapchain {
    h_dev: Arc<Device>,
    /// Copy of our images that we have allocated, so we
    /// can free them
    h_images: Vec<vk::Image>,
    h_image_mems: Vec<vk::DeviceMemory>,
}

impl HeadlessSwapchain {
    /// Return a dummy display info
    pub fn get_display_info_list(_: &Device) -> Result<Vec<Arc<dyn DisplayInfoPayload>>> {
        Ok(vec![Arc::new(HeadlessOutputPayload {})])
    }

    fn destroy_swapchain(&mut self) {
        unsafe {
            for image in self.h_images.drain(..) {
                self.h_dev.dev.destroy_image(image, None);
            }
            for mem in self.h_image_mems.drain(..) {
                self.h_dev.dev.free_memory(mem, None);
            }
        }
    }

    fn create_swapchain(&mut self, dstate: &mut DisplayState) {
        assert!(dstate.d_images.len() == 0);
        assert!(dstate.d_views.len() == 0);
        assert!(self.h_image_mems.len() == 0);

        let resolution = vk::Extent2D {
            width: WIDTH,
            height: HEIGHT,
        };

        for _ in 0..2 {
            let (image, view, mem) = self.h_dev.create_image(
                &resolution,
                vk::Format::B8G8R8A8_UNORM,
                vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::COLOR_ATTACHMENT,
                vk::ImageAspectFlags::COLOR,
                vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_COHERENT
                    | vk::MemoryPropertyFlags::HOST_VISIBLE,
                vk::ImageTiling::LINEAR,
            );

            dstate.d_images.push(image);
            self.h_images.push(image);
            dstate.d_views.push(view);
            self.h_image_mems.push(mem);
        }

        dstate.d_resolution = vk::Extent2D {
            width: WIDTH,
            height: HEIGHT,
        };
    }

    pub fn new(dev: Arc<Device>) -> Result<Self> {
        Ok(Self {
            h_dev: dev,
            h_images: Vec::new(),
            h_image_mems: Vec::new(),
        })
    }
}

impl Swapchain for HeadlessSwapchain {
    /// Choose a queue family
    ///
    /// returns an index into the array of queue types.
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface
    fn select_queue_family(&self) -> Result<u32> {
        let inst = &self.h_dev.inst.inst;

        // get the properties per queue family
        unsafe { inst.get_physical_device_queue_family_properties(self.h_dev.pdev) }
            // for each property info
            .iter()
            .enumerate()
            .filter_map(|(index, info)| {
                match info.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
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
    fn get_surface_info(&self) -> Result<(vk::SurfaceCapabilitiesKHR, vk::SurfaceFormatKHR)> {
        let extent = vk::Extent2D {
            width: WIDTH,
            height: HEIGHT,
        };

        Ok((
            vk::SurfaceCapabilitiesKHR::builder()
                .min_image_count(2)
                .max_image_count(2)
                .current_extent(extent)
                .min_image_extent(extent)
                .max_image_extent(extent)
                .max_image_array_layers(1)
                .build(),
            vk::SurfaceFormatKHR::builder()
                .format(vk::Format::B8G8R8A8_UNORM)
                .color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
                .build(),
        ))
    }

    /// Recreate our swapchain.
    ///
    /// This will be done on VK_ERROR_OUT_OF_DATE_KHR, signifying that
    /// the window is being resized and we have to regenerate accordingly.
    /// Keep in mind the Pipeline in Thundr will also have to be recreated
    /// separately.
    fn recreate_swapchain(&mut self, dstate: &mut DisplayState) -> Result<()> {
        self.destroy_swapchain();
        self.create_swapchain(dstate);
        Ok(())
    }

    /// Get the Dots Per Inch for this display.
    ///
    /// For VK_KHR_display we will calculate it ourselves, and for
    /// SDL we will ask SDL to tell us it.
    fn get_dpi(&self) -> Result<(i32, i32)> {
        // Default to 100, lower end of average DPI
        Ok((100, 100))
    }

    /// Update self.current_image with the swapchain image to render to
    ///
    /// If the next image is not ready (i.e. if Vulkan returned NOT_READY or
    /// TIMEOUT), then this will loop on calling `vkAcquireNextImageKHR` until
    /// it gets a valid image. This has to be done on AMD hw or else the TIMEOUT
    /// error will get passed up the callstack and fail.
    fn get_next_swapchain_image(&mut self, dstate: &mut DisplayState) -> Result<()> {
        // simply bump the image number
        dstate.d_current_image += 1;
        if dstate.d_current_image >= self.h_images.len() as u32 {
            dstate.d_current_image = 0;
        }

        Ok(())
    }

    /// Present the current swapchain image to the screen.
    ///
    /// Finally we can actually flip the buffers and present
    /// this image.
    fn present(&mut self, _dstate: &DisplayState) -> Result<()> {
        // no-op here, nothing to present
        Ok(())
    }
}

impl Drop for HeadlessSwapchain {
    fn drop(&mut self) {
        self.destroy_swapchain();
    }
}
