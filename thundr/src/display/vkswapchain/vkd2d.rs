/// The Vulkan Direct 2 Display (VK_KHR_display) backend
///
/// This presents to physical outputs using the Vulkan extensions.
/// This has the very nice property of not requiring the DRM subsystem.
use ash::extensions::khr;
use ash::vk;
use ash::Entry;

use super::VkSwapchainBackend;
use crate::{Result as ThundrResult, SurfaceType};

/// This Display backend represents a physical monitor sitting
/// on the user's desk. It corresponds to the VK_KHR_display extension.
pub struct PhysicalDisplay {
    // the display itself
    pub display: vk::DisplayKHR,
    pub display_loader: khr::Display,
    // The physical size in millimeters
    pd_phys_dims: vk::Extent2D,
    // The native resolution of the display
    pd_native_res: vk::Extent2D,
}

impl PhysicalDisplay {
    /// Create an on-screen surface.
    ///
    /// This will grab the function pointer loaders for the
    /// surface and display extensions and then create a
    /// surface to be rendered to.
    pub(crate) unsafe fn new(
        entry: &Entry,
        inst: &ash::Instance,
        pdev: vk::PhysicalDevice,
        surface_loader: &khr::Surface,
        surf_type: &SurfaceType,
    ) -> Option<(Box<dyn VkSwapchainBackend>, vk::SurfaceKHR, vk::Extent2D)> {
        let d_loader = khr::Display::new(entry, inst);
        let disp_props = d_loader
            .get_physical_device_display_properties(pdev)
            .unwrap();

        let ret = Box::new(PhysicalDisplay {
            display_loader: d_loader,
            display: disp_props[0].display,
            pd_phys_dims: disp_props[0].physical_dimensions,
            pd_native_res: disp_props[0].physical_resolution,
        });
        let surface = ret
            .create_surface(entry, inst, pdev, surface_loader, surf_type)
            .unwrap();
        let caps = surface_loader
            .get_physical_device_surface_capabilities(pdev, surface)
            .unwrap();

        Some((ret, surface, caps.current_extent))
    }
}

impl VkSwapchainBackend for PhysicalDisplay {
    /// Get a physical display surface.
    ///
    /// This returns the surfaceKHR to create a swapchain with, the
    /// mode the display is using, and the resolution of the screen.
    /// The resolution is returned here to avoid having to recall the
    /// vkGetDisplayModeProperties function a second time.
    ///
    /// Yea this has a gross amount of return values...
    fn create_surface(
        &self,
        _entry: &Entry,        // entry and inst aren't used but still need
        _inst: &ash::Instance, // to be passed for compatibility
        pdev: vk::PhysicalDevice,
        _surface_loader: &khr::Surface,
        _surf_type: &SurfaceType,
    ) -> Result<vk::SurfaceKHR, vk::Result> {
        unsafe {
            // This is essentially a list of the available displays.
            // Despite having a display_name member, the names are very
            // unhelpful. (e.x. "monitor").
            let disp_props = self
                .display_loader
                .get_physical_device_display_properties(pdev)
                .unwrap();

            for (i, p) in disp_props.iter().enumerate() {
                println!("{} display: {:#?}", i, p);
            }

            // The available modes for the display. This holds
            // the resolution.
            let mode_props = self
                .display_loader
                .get_display_mode_properties(pdev, self.display)
                .unwrap();

            for (i, m) in mode_props.iter().enumerate() {
                println!("display 0 - {} mode: {:#?}", i, m);
            }

            // As of now we are not doing anything important with planes,
            // but it is still useful to see which ones are reported by
            // the hardware.
            let plane_props = self
                .display_loader
                .get_physical_device_display_plane_properties(pdev)
                .unwrap();

            for (i, p) in plane_props.iter().enumerate() {
                println!("display 0 - plane: {} props = {:#?}", i, p);

                let supported = self
                    .display_loader
                    .get_display_plane_supported_displays(pdev, 0) // plane index
                    .unwrap();

                for (i, d) in disp_props.iter().enumerate() {
                    if supported.contains(&d.display) {
                        println!("  supports display {}", i);
                    }
                }
            }

            // create a display mode from the parameters we got earlier
            let mode_info =
                vk::DisplayModeCreateInfoKHR::builder().parameters(mode_props[0].parameters);
            let mode = self
                .display_loader
                .create_display_mode(pdev, self.display, &mode_info, None)
                .unwrap();

            // Print out the plane capabilities
            for (i, _) in plane_props.iter().enumerate() {
                let caps = self
                    .display_loader
                    .get_display_plane_capabilities(pdev, mode, i as u32)
                    .unwrap();
                println!("Plane {}: supports alpha {:?}", i, caps.supported_alpha);
            }

            // Finally we can create our surface to render to. From this
            // point on everything is normal
            let surf_info = vk::DisplaySurfaceCreateInfoKHR::builder()
                .display_mode(mode)
                // TODO: Don't just chose the first plane
                .plane_index(0)
                // TODO: check plane_props to make sure identity is set
                .transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
                .alpha_mode(vk::DisplayPlaneAlphaFlagsKHR::OPAQUE)
                .image_extent(mode_props[0].parameters.visible_region);

            match self
                .display_loader
                .create_display_plane_surface(&surf_info, None)
            {
                Ok(surf) => Ok(surf),
                Err(e) => Err(e),
            }
        }
    }

    fn get_dpi(&self) -> ThundrResult<(i32, i32)> {
        let dpi_h = self.pd_native_res.width / self.pd_phys_dims.width;
        let dpi_v = self.pd_native_res.height / self.pd_phys_dims.height;

        Ok((dpi_h as i32, dpi_v as i32))
    }

    fn get_vulkan_drawable_size(&self) -> Option<vk::Extent2D> {
        None
    }
}
