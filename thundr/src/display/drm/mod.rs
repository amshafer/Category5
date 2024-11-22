/// DRM backend
///
/// Austin Shafer - 2024
pub mod drm_device;
use drm_device::DrmDevice;
mod blob;

extern crate drm;
extern crate gbm;
use ash::vk;
use drm::control::{
    atomic, connector, crtc, framebuffer, plane, property, Device as ControlDevice,
};
use drm::{control, Device as DrmDeviceTrait};

use super::{DisplayState, Swapchain};
use crate::device::Device;
use crate::image::{Dmabuf, DmabufPlane};
use crate::{Result, ThundrError};
use utils::log;

use std::os::fd::AsFd;
use std::sync::Arc;

// Constants to use to index for the property handles. We do this
// instead of using a string search hashmap repeatedly.
const ACTIVE: usize = 0;
const FB_ID: usize = 1;
const CRTC_ID: usize = 2;
const SRC_X: usize = 3;
const SRC_Y: usize = 4;
const SRC_W: usize = 5;
const SRC_H: usize = 6;
const CRTC_X: usize = 7;
const CRTC_Y: usize = 8;
const CRTC_W: usize = 9;
const CRTC_H: usize = 10;
const MODE_ID: usize = 11;

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
    /// DRM plane we are presenting to. Should be primary
    ds_plane: plane::Handle,
    /// Our ARGB8888 supported modifiers
    ds_plane_mods: Vec<drm::buffer::DrmModifier>,
    /// Our plane properties. This is indexed by the constants
    /// above instead of using a HashMap provided by drm-rs
    ds_props: Vec<property::Handle>,
    /// Our DRM CRTC
    ds_crtc: crtc::Info,
    /// Our DRM Connector
    ds_conn: connector::Info,
    /// The index of the current mode in ds_conn
    ds_current_mode: usize,
    /// GBM Buffer objects
    ds_gbm_bos: Vec<gbm::BufferObject<()>>,
    /// DRM Framebuffers
    ds_fbs: Vec<framebuffer::Handle>,
    /// Vulkan representation of the above bos and fbs
    ds_images: Vec<vk::Image>,
    ds_image_mems: Vec<vk::DeviceMemory>,
    /// Have we committed yet, i.e. should we wait for flip?
    ds_committed: bool,
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

        let drm = self.ds_dev.d_drm_node.as_ref().unwrap().lock().unwrap();
        for fb in self.ds_fbs.drain(..) {
            drm.destroy_framebuffer(fb).unwrap();
        }

        self.ds_gbm_bos.clear();
    }

    /// This populates the framebuffers and VkImages for presentation
    fn create_swapchain(&mut self, dstate: &mut DisplayState) -> Result<()> {
        assert!(dstate.d_images.len() == 0);
        assert!(dstate.d_views.len() == 0);
        assert!(self.ds_image_mems.len() == 0);
        let drm = self.ds_dev.d_drm_node.as_ref().unwrap().lock().unwrap();

        // Default to the first (recommended) mode
        // TODO: let user choose mode
        self.ds_current_mode = 0;
        let mode = self.ds_conn.modes()[self.ds_current_mode];

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
            let bo = self
                .ds_gbm
                .create_buffer_object_with_modifiers2::<()>(
                    dstate.d_resolution.width,
                    dstate.d_resolution.height,
                    // TODO: allow other formats
                    gbm::Format::Argb8888,
                    self.ds_plane_mods.iter().copied(),
                    gbm::BufferObjectFlags::SCANOUT | gbm::BufferObjectFlags::RENDERING,
                )
                .or(Err(ThundrError::OUT_OF_MEMORY))?;

            // Now create a DRM framebuffer for our scanout buffer
            // TODO: debug drmAddFB2 here
            let fb = drm
                .add_planar_framebuffer(&bo, control::FbCmd2Flags::MODIFIERS)
                .map_err(|e| {
                    log::error!("Failed to create DRM framebuffer from GBM bo: {}", e);
                    e
                })?;

            let (image, view, mem) = Device::create_image_from_dmabuf_internal(
                &self.ds_dev,
                &Dmabuf {
                    db_width: dstate.d_resolution.width as i32,
                    db_height: dstate.d_resolution.height as i32,
                    db_planes: vec![DmabufPlane::new(
                        bo.fd().or(Err(ThundrError::INVALID_FD))?,      // dmabuf
                        0,                                              // plane
                        bo.offset(0).or(Err(ThundrError::INVALID_FD))?, // offset
                        bo.stride().or(Err(ThundrError::INVALID_FD))?,  // stride
                        bo.modifier().or(Err(ThundrError::INVALID_FD))?.into(), // modifier
                    )],
                },
                vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::COLOR_ATTACHMENT,
            )
            .map_err(|e| {
                log::error!("Failed to import dmabuf from GBM: {}", e);
                e
            })?;

            self.ds_gbm_bos.push(bo);
            self.ds_fbs.push(fb);
            dstate.d_images.push(image);
            self.ds_images.push(image);
            dstate.d_views.push(view);
            self.ds_image_mems.push(mem);
        }

        Ok(())
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
        let dev_clone = dev.clone();
        let drm = dev
            .d_drm_node
            .as_ref()
            .ok_or(ThundrError::INVALID_FD)?
            .lock()
            .unwrap();

        if let Ok(0) = drm.get_driver_capability(drm::DriverCapability::AddFB2Modifiers) {
            log::error!("DRM driver does not support the AddFB2Modifiers capability");
            return Err(ThundrError::NO_DISPLAY);
        }

        let (res, coninfo, crtcinfo) = Self::get_drm_infos(&drm);

        // Filter each connector until we find one that's connected.
        let con = coninfo
            .iter()
            .find(|&i| i.state() == connector::State::Connected)
            .ok_or(ThundrError::NO_DISPLAY)?;

        let crtc = crtcinfo.first().ok_or(ThundrError::NO_DISPLAY)?;

        // Get a plane to present to
        let planes = drm.plane_handles().or(Err(ThundrError::NO_DISPLAY))?;
        let plane = *planes
            .iter()
            .find(|&&plane| {
                let plane_prop_list = match drm.get_properties(plane) {
                    Ok(props) => props,
                    Err(_) => return false,
                };
                let info = drm.get_plane(plane).unwrap();
                // verify this plane supports our crtc
                let compatible_crtcs = res.filter_crtcs(info.possible_crtcs());
                if !compatible_crtcs.contains(&crtc.handle()) {
                    return false;
                }

                for (&id, &val) in plane_prop_list.iter() {
                    if let Ok(prop_info) = drm.get_property(id) {
                        if prop_info
                            .name()
                            .to_str()
                            .map(|x| x == "type")
                            .unwrap_or(false)
                        {
                            return val == (drm::control::PlaneType::Primary as u32).into();
                        }
                    }
                }
                false
            })
            .ok_or(ThundrError::NO_DISPLAY)?;

        let mut props = Vec::new();

        let plane_props = drm
            .get_properties(plane)
            .or(Err(ThundrError::NO_DISPLAY))?
            .as_hashmap(&*drm)
            .or(Err(ThundrError::NO_DISPLAY))?;
        let con_props = drm
            .get_properties(con.handle())
            .or(Err(ThundrError::NO_DISPLAY))?
            .as_hashmap(&*drm)
            .or(Err(ThundrError::NO_DISPLAY))?;
        let crtc_props = drm
            .get_properties(crtc.handle())
            .or(Err(ThundrError::NO_DISPLAY))?
            .as_hashmap(&*drm)
            .or(Err(ThundrError::NO_DISPLAY))?;

        // This order must follow the order of the similarly named constants
        props.push(crtc_props["ACTIVE"].handle());
        props.push(plane_props["FB_ID"].handle());
        props.push(con_props["CRTC_ID"].handle());
        props.push(plane_props["SRC_X"].handle());
        props.push(plane_props["SRC_Y"].handle());
        props.push(plane_props["SRC_W"].handle());
        props.push(plane_props["SRC_H"].handle());
        props.push(plane_props["CRTC_X"].handle());
        props.push(plane_props["CRTC_Y"].handle());
        props.push(plane_props["CRTC_W"].handle());
        props.push(plane_props["CRTC_H"].handle());
        props.push(crtc_props["MODE_ID"].handle());

        // Filter a list of supported modifiers
        let render_mods = dev_clone.get_supported_drm_render_modifiers();
        let mut mods = blob::get_argb8888_modifiers(&drm, plane)?;
        mods.retain(|modifier| {
            // Find our modifier in our render modifier list
            let rmod = match render_mods
                .iter()
                .find(|m| m.drm_format_modifier == (*modifier).into())
            {
                Some(m) => m,
                None => return false,
            };

            // If it has more than one plane we don't support it
            rmod.drm_format_modifier_plane_count == 1
        });

        let gbm = gbm::Device::new(drm.as_fd().try_clone_to_owned()?).map_err(|e| {
            log::error!("Could not create GBM Device: {:?}", e);
            e
        })?;

        Ok(Self {
            ds_dev: dev_clone,
            ds_gbm: gbm,
            ds_plane: plane,
            ds_plane_mods: mods,
            ds_props: props,
            ds_conn: con.clone(),
            // Default to the first (recommended) mode
            ds_current_mode: 0,
            ds_crtc: crtc.clone(),
            ds_gbm_bos: Vec::new(),
            ds_fbs: Vec::new(),
            ds_images: Vec::new(),
            ds_image_mems: Vec::new(),
            ds_committed: false,
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
        let mode = self.ds_conn.modes()[self.ds_current_mode];
        let (disp_width, disp_height) = mode.size();
        let extent = vk::Extent2D {
            width: disp_width as u32,
            height: disp_height as u32,
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
        self.create_swapchain(dstate)?;
        Ok(())
    }

    /// Get the Dots Per Inch for this display.
    ///
    /// Do this by getting the physical dimensions of the DRM connector in use
    /// and dividing the preferred mode by them.
    fn get_dpi(&self) -> Result<(i32, i32)> {
        // size of display in mm
        let physical_size = self.ds_conn.size().ok_or(ThundrError::NO_DISPLAY)?;
        // Get the resolution of the native mode
        // use the first mode, which is assumed to be the "ideal" one
        let mode = self.ds_conn.modes()[0];
        let (disp_width, disp_height) = mode.size();

        let dpi_h = disp_width as u32 / physical_size.0;
        let dpi_v = disp_height as u32 / physical_size.1;

        Ok((dpi_h as i32, dpi_v as i32))
    }

    /// Update self.current_image with the swapchain image to render to
    ///
    /// This will wait for the previous atomic commit's flip event to fire
    /// before updating our current image and continuing.
    fn get_next_swapchain_image(&mut self, dstate: &mut DisplayState) -> Result<()> {
        log::debug!("get_next_swapchain_image: enter");

        if self.ds_committed {
            // Wait for an event saying the previous atomic commit has been
            // applied
            //
            // There may be multiple DrmSwapchains using us to wait for flip events. If we
            // are processing a particular CRTC then we will cache flip events for other
            // CRTCs so they can find them.
            loop {
                // First check the available event list. If there is an event for our CRTC
                // then we remove it and are good to go.
                let mut drm_events = self.ds_dev.d_drm_events.lock().unwrap();
                if let Some(index) = drm_events
                    .iter()
                    .position(|flip| flip.crtc == self.ds_crtc.handle())
                {
                    drm_events.remove(index);
                    break;
                }

                // If there was no pending flip, then acquire the DrmDevice and wait for
                // new events. If our CRTC was found we are good to go, record any others
                // in the pending events list
                let drm = self.ds_dev.d_drm_node.as_ref().unwrap().lock().unwrap();

                let events = drm.receive_events().map_err(|e| {
                    log::debug!("Failed to get DRM events: {:?}", e);
                    ThundrError::COULD_NOT_ACQUIRE_NEXT_IMAGE
                })?;

                let mut flip_event_found = false;
                for ev in events {
                    if let control::Event::PageFlip(flip) = ev {
                        // Record all events except for our CRTC
                        match flip.crtc == self.ds_crtc.handle() {
                            true => flip_event_found = true,
                            false => drm_events.push(flip),
                        }
                    }
                }

                // We found our flip event, now we can exit
                if flip_event_found {
                    self.ds_committed = false;
                    break;
                }
            }
        }
        log::debug!("get_next_swapchain_image: got image");

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
    fn present(&mut self, dstate: &DisplayState) -> Result<()> {
        log::debug!("present: enter");
        // First wait for rendering to complete
        self.ds_dev.wait_for_latest_timeline();
        log::debug!("present: waited for rendering");

        // Now create an atomic commit with our latest frame
        let drm = self.ds_dev.d_drm_node.as_ref().unwrap().lock().unwrap();
        let mode = self.ds_conn.modes()[self.ds_current_mode];

        let mut atomic_req = atomic::AtomicModeReq::new();
        atomic_req.add_property(
            self.ds_conn.handle(),
            self.ds_props[CRTC_ID],
            property::Value::CRTC(Some(self.ds_crtc.handle())),
        );
        let blob = drm
            .create_property_blob(&mode)
            .expect("Failed to create blob");
        atomic_req.add_property(self.ds_crtc.handle(), self.ds_props[MODE_ID], blob);
        atomic_req.add_property(
            self.ds_crtc.handle(),
            self.ds_props[ACTIVE],
            property::Value::Boolean(true),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[FB_ID],
            property::Value::Framebuffer(Some(self.ds_fbs[dstate.d_current_image as usize])),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[CRTC_ID],
            property::Value::CRTC(Some(self.ds_crtc.handle())),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[SRC_X],
            property::Value::UnsignedRange(0),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[SRC_Y],
            property::Value::UnsignedRange(0),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[SRC_W],
            property::Value::UnsignedRange((mode.size().0 as u64) << 16),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[SRC_H],
            property::Value::UnsignedRange((mode.size().1 as u64) << 16),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[CRTC_X],
            property::Value::SignedRange(0),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[CRTC_Y],
            property::Value::SignedRange(0),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[CRTC_W],
            property::Value::UnsignedRange(mode.size().0 as u64),
        );
        atomic_req.add_property(
            self.ds_plane,
            self.ds_props[CRTC_H],
            property::Value::UnsignedRange(mode.size().1 as u64),
        );

        // Set the crtc
        // On many setups, this requires root access.
        let ret = drm
            .atomic_commit(
                control::AtomicCommitFlags::ALLOW_MODESET
                    | control::AtomicCommitFlags::NONBLOCK
                    | control::AtomicCommitFlags::PAGE_FLIP_EVENT,
                atomic_req,
            )
            .or(Err(ThundrError::PRESENT_FAILED));
        self.ds_committed = true;
        log::debug!("present: done with flip");

        ret
    }
}
