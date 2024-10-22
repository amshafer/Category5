/// DRM backend
///
/// Austin Shafer - 2024
pub mod drm_device;
use drm_device::DrmDevice;

extern crate drm;
extern crate gbm;
use ash::vk;
use drm::buffer::DrmFourcc;
use drm::control::{connector, crtc, encoder, framebuffer, Device as ControlDevice};

use super::{DisplayState, Swapchain};
use crate::device::Device;
use crate::{Result, ThundrError};
use utils::log;

use std::os::fd::AsFd;
use std::sync::Arc;

/// A thundr backend which uses linux's DRM KMS
///
/// This allows for fine-grained display control and is the
/// optimal display method for compositors. This uses the
/// atomic DRM api. It drives one connector on the device,
/// and handles swapchain management for that output.
pub struct DrmSwapchain {
    /// Our DRM KMS node
    ds_dev: Arc<Device>,
    /// Our gbm_device
    ds_gbm: gbm::Device<std::os::fd::OwnedFd>,
    /// Our DRM CRTC
    ds_crtc: crtc::Info,
    /// Our DRM Connector
    ds_conn: connector::Info,
    /// The index of the current mode in ds_conn
    ds_current_mode: usize,
    /// Vulkan representation of scanout images
    ds_images: Vec<vk::Image>,
    ds_image_mems: Vec<vk::DeviceMemory>,
}

impl DrmSwapchain {
    fn destroy_swapchain(&mut self) {
        unsafe {
            for image in self.ds_images.drain(..) {
                self.ds_dev.dev.destroy_image(image, None);
            }
            for mem in self.ds_image_mems.drain(..) {
                self.ds_dev.dev.free_memory(mem, None);
            }
        }
    }

    /// This populates the framebuffers and VkImages for presentation
    fn create_swapchain(&mut self, dstate: &mut DisplayState) {
        assert!(dstate.d_images.len() == 0);
        assert!(dstate.d_views.len() == 0);
        assert!(self.ds_image_mems.len() == 0);

        // Default to the first (recommended) mode
        // TODO: let user choose mode
        self.ds_current_mode = 0;
        let mode = self.ds_conn.modes()[self.ds_current_mode];
        // TODO: allow other formats
        let fmt = DrmFourcc::Xrgb8888;

        let (disp_width, disp_height) = mode.size();
        dstate.d_resolution = vk::Extent2D {
            width: disp_width as u32,
            height: disp_height as u32,
        };

        // Now create our swapchain images
        //
        // For this we are going to create a set of DRM Framebuffers, and then import that
        // memory into Vulkan for the rest of Thundr to use.
        for _ in 0..2 {
            let (image, view, mem) = self.ds_dev.create_image(
                &dstate.d_resolution,
                vk::Format::B8G8R8A8_UNORM,
                vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::COLOR_ATTACHMENT,
                vk::ImageAspectFlags::COLOR,
                vk::MemoryPropertyFlags::DEVICE_LOCAL
                    | vk::MemoryPropertyFlags::HOST_COHERENT
                    | vk::MemoryPropertyFlags::HOST_VISIBLE,
                vk::ImageTiling::LINEAR,
            );

            dstate.d_images.push(image);
            self.ds_images.push(image);
            dstate.d_views.push(view);
            self.ds_image_mems.push(mem);
        }
    }

    /// Helper to get all of the DRM handles
    fn get_drm_infos(
        drm: &DrmDevice,
    ) -> (
        drm::control::ResourceHandles,
        Vec<connector::Info>,
        Vec<crtc::Info>,
    ) {
        let res = drm.resource_handles().unwrap();
        let coninfo: Vec<connector::Info> = res
            .connectors()
            .iter()
            .flat_map(|con| drm.get_connector(*con, true))
            .collect();
        let crtcinfo: Vec<crtc::Info> = res
            .crtcs()
            .iter()
            .flat_map(|crtc| drm.get_crtc(*crtc))
            .collect();

        (res, coninfo, crtcinfo)
    }

    /// Create a new DRM swapchain for this device, requesting a new connector.
    ///
    /// Returns INVALID_FD if no DRM node is in use. Returns NO_DISPLAY if
    /// there are no available connectors.
    pub fn new(dev: Arc<Device>) -> Result<Self> {
        let drm = dev.d_drm_node.as_ref().ok_or(ThundrError::INVALID_FD)?;

        let (_res, coninfo, crtcinfo) = Self::get_drm_infos(&drm);

        // Filter each connector until we find one that's connected.
        let con = coninfo
            .iter()
            .find(|&i| i.state() == connector::State::Connected)
            .ok_or(ThundrError::NO_DISPLAY)?;

        let crtc = crtcinfo.first().ok_or(ThundrError::NO_DISPLAY)?;

        let gbm = gbm::Device::new(drm.as_fd().try_clone_to_owned()?).map_err(|e| {
            log::error!("Could not create GBM Device: {:?}", e);
            e
        })?;

        Ok(Self {
            ds_dev: dev,
            ds_gbm: gbm,
            ds_conn: con.clone(),
            // Default to the first (recommended) mode
            ds_current_mode: 0,
            ds_crtc: crtc.clone(),
            ds_images: Vec::new(),
            ds_image_mems: Vec::new(),
        })
    }
}

impl Swapchain for DrmSwapchain {
    /// Choose a queue family
    ///
    /// returns an index into the array of queue types.
    /// provide the surface PFN loader and the surface so
    /// that we can ensure the pdev/queue combination can
    /// present the surface
    fn select_queue_family(&self) -> Result<u32> {
        let inst = &self.ds_dev.inst.inst;

        // get the properties per queue family
        unsafe { inst.get_physical_device_queue_family_properties(self.ds_dev.pdev) }
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
    fn get_dpi(&self) -> Result<(i32, i32)> {}

    /// Update self.current_image with the swapchain image to render to
    ///
    /// If the next image is not ready (i.e. if Vulkan returned NOT_READY or
    /// TIMEOUT), then this will loop on calling `vkAcquireNextImageKHR` until
    /// it gets a valid image. This has to be done on AMD hw or else the TIMEOUT
    /// error will get passed up the callstack and fail.
    fn get_next_swapchain_image(&mut self, dstate: &mut DisplayState) -> Result<()> {
        // bump the image number
        dstate.d_current_image += 1;
        if dstate.d_current_image >= self.ds_images.len() as u32 {
            dstate.d_current_image = 0;
        }

        Ok(())
    }

    /// Present the current swapchain image to the screen.
    ///
    /// Finally we can actually flip the buffers and present
    /// this image.
    fn present(&mut self, dstate: &DisplayState) -> Result<()> {}
}
