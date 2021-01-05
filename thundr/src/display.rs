// The Display object owned by Renderer
//
// Austin Shafer - 2020

#![allow(dead_code, non_camel_case_types)]
extern crate ash;

#[cfg(feature = "xlib")]
extern crate winit;

use ash::extensions::ext::DebugReport;
use ash::extensions::khr;
use ash::version::{EntryV1_0, InstanceV1_0};
use ash::vk;

use crate::{CreateInfo, SurfaceType};

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
    // the actual surface (KHR extension)
    pub d_surface: vk::SurfaceKHR,
    // function pointer loaders
    pub d_surface_loader: khr::Surface,
    pub d_resolution: vk::Extent2D,
    d_back: Backend,
}

enum Backend {
    PhysicalDisplay(PhysicalDisplay),
    #[cfg(feature = "xlib")]
    XlibDisplay(XlibDisplay),
}

enum BackendType {
    PhysicalDisplay,
    #[cfg(feature = "xlib")]
    XlibDisplay,
}

impl Display {
    fn choose_display_backend(info: &CreateInfo) -> BackendType {
        match info.surface_type {
            SurfaceType::Display => BackendType::PhysicalDisplay,
            #[cfg(feature = "xlib")]
            SurfaceType::Xlib(_) => BackendType::XlibDisplay,
        }
    }

    pub unsafe fn new<E: EntryV1_0, I: InstanceV1_0>(
        info: &CreateInfo,
        entry: &E,
        inst: &I,
        pdev: vk::PhysicalDevice,
    ) -> Display {
        let s_loader = khr::Surface::new(entry, inst);
        let (back, surf, res) = match info.surface_type {
            SurfaceType::Display => {
                let n = PhysicalDisplay::new(entry, inst, pdev);
                (Backend::PhysicalDisplay(n.0), n.1, n.2)
            }
            #[cfg(feature = "xlib")]
            SurfaceType::Xlib(win) => {
                let (xd, surf) = XlibDisplay::new(entry, inst, pdev, &win);
                let caps = s_loader
                    .get_physical_device_surface_capabilities(pdev, surf)
                    .unwrap();
                (Backend::XlibDisplay(xd), surf, caps.current_extent)
            }
        };

        Self {
            d_surface_loader: s_loader,
            d_back: back,
            d_surface: surf,
            d_resolution: res,
        }
    }

    /// Selects a resolution for the renderer
    ///
    /// We saved the resolution of the display surface when we created
    /// it. If the surface capabilities doe not specify a requested
    /// extent, then we will return the screen's resolution.
    pub unsafe fn select_resolution(
        &self,
        surface_caps: &vk::SurfaceCapabilitiesKHR,
    ) -> vk::Extent2D {
        match surface_caps.current_extent.width {
            std::u32::MAX => self.d_resolution,
            _ => surface_caps.current_extent,
        }
    }

    pub unsafe fn select_surface_format(&self, pdev: vk::PhysicalDevice) -> vk::SurfaceFormatKHR {
        match &self.d_back {
            Backend::PhysicalDisplay(pd) => {
                pd.select_surface_format(&self.d_surface_loader, self.d_surface, pdev)
            }
        }
    }

    pub fn extension_names(info: &CreateInfo) -> Vec<*const i8> {
        match Self::choose_display_backend(info) {
            BackendType::PhysicalDisplay => PhysicalDisplay::extension_names(),
            #[cfg(feature = "xlib")]
            BackendType::XlibDisplay => XlibDisplay::extension_names(),
        }
    }

    pub fn destroy(&mut self) {
        println!("Destroying display");
        unsafe {
            self.d_surface_loader.destroy_surface(self.d_surface, None);
        }
        // It seems that the display resources (mode) are cleaned up
        // when the surface is destroyed. There are not separate
        // deconstructors for them
        //
        // The validation layers do warn about them however (bug?)
    }
}

/// This Display backend represents a physical monitor sitting
/// on the user's desk. It corresponds to the VK_KHR_display extension.
struct PhysicalDisplay {
    // the display itself
    pub display: vk::DisplayKHR,
    // The mode the display was created with
    pub display_mode: vk::DisplayModeKHR,
    pub display_loader: khr::Display,
}

impl PhysicalDisplay {
    /// Create an on-screen surface.
    ///
    /// This will grab the function pointer loaders for the
    /// surface and display extensions and then create a
    /// surface to be rendered to.
    unsafe fn new<E: EntryV1_0, I: InstanceV1_0>(
        entry: &E,
        inst: &I,
        pdev: vk::PhysicalDevice,
    ) -> (Self, vk::SurfaceKHR, vk::Extent2D) {
        let d_loader = khr::Display::new(entry, inst);

        let (display, surface, mode, resolution) =
            PhysicalDisplay::create_surface(entry, inst, &d_loader, pdev).unwrap();

        let ret = PhysicalDisplay {
            display_loader: d_loader,
            display_mode: mode,
            display: display,
        };

        (ret, surface, resolution)
    }

    /// choose a vkSurfaceFormatKHR for the vkSurfaceKHR
    ///
    /// This selects the color space and layout for a surface. This should
    /// be called by the Renderer after creating a Display.
    unsafe fn select_surface_format(
        &self,
        surface_loader: &khr::Surface,
        surface: vk::SurfaceKHR,
        pdev: vk::PhysicalDevice,
    ) -> vk::SurfaceFormatKHR {
        let formats = surface_loader
            .get_physical_device_surface_formats(pdev, surface)
            .unwrap();

        formats
            .iter()
            .map(|fmt| match fmt.format {
                // if the surface does not specify a desired format
                // then we can choose our own
                vk::Format::UNDEFINED => vk::SurfaceFormatKHR {
                    format: vk::Format::R8G8B8A8_UNORM,
                    color_space: fmt.color_space,
                },
                // if the surface has a desired format we will just
                // use that
                _ => *fmt,
            })
            .nth(0)
            .expect("Could not find a surface format")
    }

    /// Get a physical display surface.
    ///
    /// This returns the surfaceKHR to create a swapchain with, the
    /// mode the display is using, and the resolution of the screen.
    /// The resolution is returned here to avoid having to recall the
    /// vkGetDisplayModeProperties function a second time.
    ///
    /// Yea this has a gross amount of return values...
    #[cfg(unix)]
    unsafe fn create_surface<E: EntryV1_0, I: InstanceV1_0>(
        _entry: &E, // entry and inst aren't used but still need
        _inst: &I,  // to be passed for compatibility
        loader: &khr::Display,
        pdev: vk::PhysicalDevice,
    ) -> Result<
        (
            vk::DisplayKHR,
            vk::SurfaceKHR,
            vk::DisplayModeKHR,
            vk::Extent2D,
        ),
        vk::Result,
    > {
        // This is essentially a list of the available displays.
        // Despite having a display_name member, the names are very
        // unhelpful. (e.x. "monitor").
        let disp_props = loader.get_physical_device_display_properties(pdev).unwrap();

        for (i, p) in disp_props.iter().enumerate() {
            println!("{} display: {:#?}", i, p);
        }

        // The available modes for the display. This holds
        // the resolution.
        let mode_props = loader
            .get_display_mode_properties(pdev, disp_props[0].display)
            .unwrap();

        for (i, m) in mode_props.iter().enumerate() {
            println!("display 0 - {} mode: {:#?}", i, m);
        }

        // As of now we are not doing anything important with planes,
        // but it is still useful to see which ones are reported by
        // the hardware.
        let plane_props = loader
            .get_physical_device_display_plane_properties(pdev)
            .unwrap();

        for (i, p) in plane_props.iter().enumerate() {
            println!("display 0 - plane: {} props = {:#?}", i, p);

            let supported = loader
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
        let mode = loader
            .create_display_mode(pdev, disp_props[0].display, &mode_info, None)
            .unwrap();

        // Print out the plane capabilities
        for (i, _) in plane_props.iter().enumerate() {
            let caps = loader
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

        match loader.create_display_plane_surface(&surf_info, None) {
            // we want to return the display, the surface, the mode
            // (so we can free it later), and the resolution to be saved.
            Ok(surf) => Ok((
                disp_props[0].display,
                surf,
                mode,
                mode_props[0].parameters.visible_region,
            )),
            Err(e) => Err(e),
        }
    }

    /// this should really go in its own Platform module
    ///
    /// The two most important extensions are Surface and Display.
    /// Without them we cannot render anything.
    fn extension_names() -> Vec<*const i8> {
        vec![
            khr::Surface::name().as_ptr(),
            khr::Display::name().as_ptr(),
            DebugReport::name().as_ptr(),
        ]
    }
}

#[cfg(feature = "xlib")]
struct XlibDisplay {
    // the display itself
    pub xlib_loader: khr::XlibSurface,
}

#[cfg(feature = "xlib")]
impl XlibDisplay {
    /// Create an on-screen surface.
    ///
    /// This will grab the function pointer loaders for the
    /// surface and display extensions and then create a
    /// surface to be rendered to.
    unsafe fn new<E: EntryV1_0, I: InstanceV1_0>(
        entry: &E,
        inst: &I,
        pdev: vk::PhysicalDevice,
        win: &winit::window::Window,
    ) -> (Self, vk::SurfaceKHR) {
        let x_loader = khr::XlibSurface::new(entry, inst);

        let surface = Self::create_surface(entry, inst, &x_loader, pdev, win).unwrap();

        let ret = Self {
            xlib_loader: x_loader,
        };

        (ret, surface)
    }

    /// choose a vkSurfaceFormatKHR for the vkSurfaceKHR
    unsafe fn select_surface_format(
        &self,
        surface_loader: &khr::Surface,
        surface: vk::SurfaceKHR,
        pdev: vk::PhysicalDevice,
    ) -> vk::SurfaceFormatKHR {
        let formats = surface_loader
            .get_physical_device_surface_formats(pdev, surface)
            .unwrap();

        formats
            .iter()
            .map(|fmt| match fmt.format {
                // if the surface does not specify a desired format
                // then we can choose our own
                vk::Format::UNDEFINED => vk::SurfaceFormatKHR {
                    format: vk::Format::R8G8B8A8_UNORM,
                    color_space: fmt.color_space,
                },
                // if the surface has a desired format we will just
                // use that
                _ => *fmt,
            })
            .nth(0)
            .expect("Could not find a surface format")
    }

    /// Get an x11 display surface.
    #[cfg(unix)]
    unsafe fn create_surface<E: EntryV1_0, I: InstanceV1_0>(
        entry: &E, // entry and inst aren't used but still need
        inst: &I,  // to be passed for compatibility
        loader: &khr::XlibSurface,
        pdev: vk::PhysicalDevice,
        win: &winit::window::Window,
    ) -> Result<vk::SurfaceKHR, vk::Result> {
        use winit::platform::unix::WindowExtUnix;
        let x11_display = win.xlib_display().unwrap();
        let x11_window = win.xlib_window().unwrap();
        let x11_create_info = vk::XlibSurfaceCreateInfoKHR::builder()
            .window(x11_window)
            .dpy(x11_display as *mut vk::Display);

        let xlib_surface_loader = khr::XlibSurface::new(entry, inst);
        loader.create_xlib_surface(&x11_create_info, None)
    }

    /// The two most important extensions are Surface and Xlib.
    fn extension_names() -> Vec<*const i8> {
        vec![
            khr::Surface::name().as_ptr(),
            khr::XlibSurface::name().as_ptr(),
            DebugReport::name().as_ptr(),
        ]
    }
}
