///! SDL2 backend for generic window system controls
///
/// Austin Shafer - 2024
extern crate sdl2;

use ash::extensions::khr;
use ash::vk;
use ash::Entry;

use super::VkSwapchainBackend;
use crate::{Result as ThundrResult, WindowInfo};
use utils::log;

/// The SDL backend is the general purpose window system
/// glue backend. It should work most places as that is what
/// SDL is meant for, but with the downside that it's always
/// a little wonky at certain things.
pub struct SDL2DisplayBackend {
    sdl_window: sdl2::video::Window,
    sdl_video: sdl2::VideoSubsystem,
}

impl SDL2DisplayBackend {
    /// Create an on-screen surface.
    ///
    /// This will grab the function pointer loaders for the
    /// surface and display extensions and then create a
    /// surface to be rendered to.
    pub(crate) fn new(
        entry: &Entry,
        inst: &ash::Instance,
        pdev: vk::PhysicalDevice,
        surface_loader: &khr::Surface,
        win_info: &WindowInfo,
    ) -> Option<(Box<dyn VkSwapchainBackend>, vk::SurfaceKHR, vk::Extent2D)> {
        match win_info {
            WindowInfo::SDL2(vid_sys, win) => {
                let ret = Box::new(Self {
                    // create a new window wrapper by cloning the Rc pointer
                    sdl_window: unsafe { sdl2::video::Window::from_ref(win.context()) },
                    sdl_video: (*vid_sys).clone(),
                });

                let surface = ret
                    .create_surface(entry, inst, pdev, surface_loader, win_info)
                    .unwrap();
                let caps = unsafe {
                    surface_loader
                        .get_physical_device_surface_capabilities(pdev, surface)
                        .unwrap()
                };

                Some((ret, surface, caps.current_extent))
            }
            _ => None,
        }
    }
}

impl VkSwapchainBackend for SDL2DisplayBackend {
    /// Get an x11 display surface.
    fn create_surface(
        &self,
        _entry: &Entry,       // entry and inst aren't used but still need
        inst: &ash::Instance, // to be passed for compatibility
        _pdev: vk::PhysicalDevice,
        _surface_loader: &khr::Surface,
        win_info: &WindowInfo,
    ) -> Result<vk::SurfaceKHR, vk::Result> {
        use vk::Handle;

        match win_info {
            WindowInfo::SDL2(_, win) => {
                // we need to convert our ash instance into the pointer to the raw vk instance
                let raw_surf = match win.vulkan_create_surface(inst.handle().as_raw() as usize) {
                    Ok(s) => s,
                    Err(s) => {
                        log::error!("SDL2 vulkan_create_surface failed: {}", s);
                        return Err(vk::Result::ERROR_UNKNOWN);
                    }
                };

                Ok(vk::SurfaceKHR::from_raw(raw_surf))
            }
            _ => panic!("Trying to create SDL backend on non-SDL surface"),
        }
    }

    fn get_dpi(&self) -> ThundrResult<(i32, i32)> {
        let dpi = self
            .sdl_video
            .display_dpi(self.sdl_window.display_index().unwrap())
            .or(Err(crate::ThundrError::INVALID))?;

        // Scale the reported DPI by the scaling factor
        let win_size = self.sdl_window.size();
        let vk_size = self.sdl_window.vulkan_drawable_size();

        // return hdpi and vdpi
        let ret = Ok((
            dpi.1 as i32 * (win_size.0 as i32 / vk_size.0 as i32),
            dpi.2 as i32 * (win_size.1 as i32 / vk_size.1 as i32),
        ));

        log::info!("Final DPI: {:?}", ret);
        return ret;
    }

    fn get_vulkan_drawable_size(&self) -> Option<vk::Extent2D> {
        //let res = self.sdl_window.vulkan_drawable_size();
        //Some(vk::Extent2D {
        //    width: res.0,
        //    height: res.1,
        //})
        None
    }
}
