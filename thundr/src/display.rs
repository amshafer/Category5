// The Display object owned by Renderer
//
// Austin Shafer - 2020

#![allow(dead_code, non_camel_case_types)]
extern crate ash;

#[cfg(feature = "sdl")]
extern crate sdl2;

#[cfg(feature = "wayland")]
extern crate wayland_client as wc;

use ash::extensions::khr;
use ash::vk;
use ash::{Entry, Instance};

use crate::pipelines::PipelineType;
use crate::{CreateInfo, Result as ThundrResult, SurfaceType, ThundrError};
use utils::log;

use std::str::FromStr;

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
    d_back: Box<dyn Backend>,
    /// Cache the present mode here so we don't re-request it
    pub d_present_mode: vk::PresentModeKHR,
}

trait Backend {
    /// Get an x11 display surface.
    unsafe fn create_surface(
        &self,
        entry: &Entry,   // entry and inst aren't used but still need
        inst: &Instance, // to be passed for compatibility
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
    #[cfg(feature = "wayland")]
    WaylandDisplay,
}

impl Display {
    fn choose_display_backend(info: &CreateInfo) -> BackendType {
        match info.surface_type {
            SurfaceType::Display(_) => BackendType::PhysicalDisplay,
            #[cfg(feature = "sdl")]
            SurfaceType::SDL2(_, _) => BackendType::SDL2Display,
            #[cfg(feature = "wayland")]
            SurfaceType::Wayland(_, _) => BackendType::WaylandDisplay,
        }
    }

    pub unsafe fn new(
        info: &CreateInfo,
        entry: &Entry,
        inst: &Instance,
        pdev: vk::PhysicalDevice,
    ) -> Display {
        let s_loader = khr::Surface::new(entry, inst);
        let (back, surf, res) = match &info.surface_type {
            SurfaceType::Display(_) => {
                PhysicalDisplay::new(entry, inst, pdev, &s_loader, &info.surface_type)
            }
            #[cfg(feature = "sdl")]
            SurfaceType::SDL2(_, _) => {
                SDL2DisplayBackend::new(entry, inst, pdev, &s_loader, &info.surface_type)
            }
            #[cfg(feature = "wayland")]
            SurfaceType::Wayland(_, _) => {
                WlDisplay::new(entry, inst, pdev, &s_loader, &info.surface_type)
            }
        }
        .unwrap();

        // the best mode for presentation is FIFO (with triple buffering)
        // as this is recommended by the samsung developer page, which
        // I am *assuming* is a good reference for low power apps
        let present_modes = s_loader
            .get_physical_device_surface_present_modes(pdev, surf)
            .unwrap();
        let mode = present_modes
            .iter()
            .cloned()
            .find(|&mode| mode == vk::PresentModeKHR::FIFO)
            // fallback to FIFO if the mailbox mode is not available
            .unwrap_or(vk::PresentModeKHR::FIFO);

        Self {
            d_surface_loader: s_loader,
            d_back: back,
            d_surface: surf,
            d_resolution: res,
            d_present_mode: mode,
        }
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
    pub unsafe fn select_resolution(
        &self,
        surface_caps: &vk::SurfaceCapabilitiesKHR,
    ) -> vk::Extent2D {
        match surface_caps.current_extent.width {
            std::u32::MAX => self.d_resolution,
            _ => surface_caps.current_extent,
        }
    }

    pub unsafe fn select_surface_format(
        &self,
        pdev: vk::PhysicalDevice,
        _pipe_type: PipelineType,
    ) -> crate::Result<vk::SurfaceFormatKHR> {
        let formats = self
            .d_surface_loader
            .get_physical_device_surface_formats(pdev, self.d_surface)
            .unwrap();
        log::error!("Formats for this vulkan surface: {:#?}", formats);

        match formats
            .iter()
            .find(|fmt| fmt.format == vk::Format::UNDEFINED)
        {
            // if the surface does not specify a desired format
            // then we can choose our own
            Some(fmt) => {
                return Ok(vk::SurfaceFormatKHR {
                    format: vk::Format::B8G8R8A8_UNORM,
                    color_space: fmt.color_space,
                })
            }
            None => {}
        };

        match formats
            .iter()
            .find(|fmt| fmt.format == vk::Format::B8G8R8A8_UNORM)
        {
            Some(fmt) => return Ok(*fmt),
            None => {}
        };

        Ok(formats[0])
    }

    pub fn extension_names(info: &CreateInfo) -> Vec<*const i8> {
        match Self::choose_display_backend(info) {
            BackendType::PhysicalDisplay => PhysicalDisplay::extension_names(&info.surface_type),
            #[cfg(feature = "sdl")]
            BackendType::SDL2Display => {
                SDL2DisplayBackend::extension_names(&info.surface_type).unwrap()
            }
            #[cfg(feature = "wayland")]
            BackendType::WaylandDisplay => WlDisplay::extension_names(&info.surface_type),
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

    pub unsafe fn get_vulkan_drawable_size(&self, pdev: vk::PhysicalDevice) -> vk::Extent2D {
        match self.d_back.get_vulkan_drawable_size() {
            Some(size) => size,
            None => {
                // If the backend doesn't support this then just get the
                // value from vulkan
                self.d_surface_loader
                    .get_physical_device_surface_capabilities(pdev, self.d_surface)
                    .expect("Could not get physical device surface capabilities")
                    .current_extent
            }
        }
    }
}

/// This Display backend represents a physical monitor sitting
/// on the user's desk. It corresponds to the VK_KHR_display extension.
struct PhysicalDisplay {
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
    unsafe fn new(
        entry: &Entry,
        inst: &Instance,
        pdev: vk::PhysicalDevice,
        surface_loader: &khr::Surface,
        surf_type: &SurfaceType,
    ) -> Option<(Box<dyn Backend>, vk::SurfaceKHR, vk::Extent2D)> {
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
                    format: vk::Format::B8G8R8A8_UNORM,
                    color_space: fmt.color_space,
                },
                // if the surface has a desired format we will just
                // use that
                _ => *fmt,
            })
            .nth(0)
            .expect("Could not find a surface format")
    }

    /// this should really go in its own Platform module
    ///
    /// The two most important extensions are Surface and Display.
    /// Without them we cannot render anything.
    fn extension_names(_surf_type: &SurfaceType) -> Vec<*const i8> {
        vec![khr::Surface::name().as_ptr(), khr::Display::name().as_ptr()]
    }
}

impl Backend for PhysicalDisplay {
    /// Get a physical display surface.
    ///
    /// This returns the surfaceKHR to create a swapchain with, the
    /// mode the display is using, and the resolution of the screen.
    /// The resolution is returned here to avoid having to recall the
    /// vkGetDisplayModeProperties function a second time.
    ///
    /// Yea this has a gross amount of return values...
    #[cfg(unix)]
    unsafe fn create_surface(
        &self,
        _entry: &Entry,   // entry and inst aren't used but still need
        _inst: &Instance, // to be passed for compatibility
        pdev: vk::PhysicalDevice,
        _surface_loader: &khr::Surface,
        _surf_type: &SurfaceType,
    ) -> Result<vk::SurfaceKHR, vk::Result> {
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

    fn get_dpi(&self) -> ThundrResult<(f32, f32)> {
        let dpi_h = self.pd_native_res.width / self.pd_phys_dims.width;
        let dpi_v = self.pd_native_res.height / self.pd_phys_dims.height;

        Ok((dpi_h as f32, dpi_v as f32))
    }

    fn get_vulkan_drawable_size(&self) -> Option<vk::Extent2D> {
        None
    }
}

#[cfg(feature = "sdl")]
struct SDL2DisplayBackend {
    sdl_window: sdl2::video::Window,
    sdl_video: sdl2::VideoSubsystem,
}

#[cfg(feature = "sdl")]
impl SDL2DisplayBackend {
    /// Create an on-screen surface.
    ///
    /// This will grab the function pointer loaders for the
    /// surface and display extensions and then create a
    /// surface to be rendered to.
    unsafe fn new(
        entry: &Entry,
        inst: &Instance,
        pdev: vk::PhysicalDevice,
        surface_loader: &khr::Surface,
        surf_type: &SurfaceType,
    ) -> Option<(Box<dyn Backend>, vk::SurfaceKHR, vk::Extent2D)> {
        match surf_type {
            SurfaceType::SDL2(vid_sys, win) => {
                let ret = Box::new(Self {
                    // create a new window wrapper by cloning the Rc pointer
                    sdl_window: sdl2::video::Window::from_ref(win.context()),
                    sdl_video: (*vid_sys).clone(),
                });

                let surface = ret
                    .create_surface(entry, inst, pdev, surface_loader, surf_type)
                    .unwrap();
                let caps = surface_loader
                    .get_physical_device_surface_capabilities(pdev, surface)
                    .unwrap();

                Some((ret, surface, caps.current_extent))
            }
            _ => None,
        }
    }

    /// The two most important extensions are Surface and Xcb.
    fn extension_names(surf_type: &SurfaceType) -> Option<Vec<*const i8>> {
        match surf_type {
            SurfaceType::SDL2(_, win) => Some(
                win.vulkan_instance_extensions()
                    .unwrap()
                    .iter()
                    .map(|s| {
                        // we need to turn a Vec<&str> into a Vec<*const i8>
                        s.as_ptr() as *const i8
                    })
                    .collect(),
            ),
            _ => None,
        }
    }
}

#[cfg(feature = "sdl")]
impl Backend for SDL2DisplayBackend {
    /// Get an x11 display surface.
    unsafe fn create_surface(
        &self,
        _entry: &Entry,  // entry and inst aren't used but still need
        inst: &Instance, // to be passed for compatibility
        _pdev: vk::PhysicalDevice,
        _surface_loader: &khr::Surface,
        surf_type: &SurfaceType,
    ) -> Result<vk::SurfaceKHR, vk::Result> {
        use vk::Handle;

        match surf_type {
            SurfaceType::SDL2(_, win) => {
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

    fn get_dpi(&self) -> ThundrResult<(f32, f32)> {
        let dpi = self
            .sdl_video
            .display_dpi(self.sdl_window.display_index().unwrap())
            .or(Err(ThundrError::INVALID))?;

        // Scale the reported DPI by the scaling factor
        let win_size = self.sdl_window.size();
        let vk_size = self.sdl_window.vulkan_drawable_size();

        // return hdpi and vdpi
        Ok((
            dpi.1 * (win_size.0 as f32 / vk_size.0 as f32),
            dpi.2 * (win_size.1 as f32 / vk_size.1 as f32),
        ))
    }

    fn get_vulkan_drawable_size(&self) -> Option<vk::Extent2D> {
        let res = self.sdl_window.vulkan_drawable_size();
        Some(vk::Extent2D {
            width: res.0,
            height: res.1,
        })
    }
}

// TODO: totally broken
#[cfg(feature = "wayland")]
struct WlDisplay {
    pub wl_loader: khr::WaylandSurface,
    wl_display: wc::Display,
    wl_surface: wc::protocol::wl_surface::WlSurface,
}

#[cfg(feature = "wayland")]
impl WlDisplay {
    /// Create an on-screen surface.
    ///
    /// This will grab the function pointer loaders for the
    /// surface and display extensions and then create a
    /// surface to be rendered to.
    unsafe fn new(
        entry: &Entry,
        inst: &Instance,
        pdev: vk::PhysicalDevice,
        wl_display: wc::Display,
        wl_surface: wc::protocol::wl_surface::WlSurface,
    ) -> (Self, vk::SurfaceKHR) {
        let wl_loader = khr::WaylandSurface::new(entry, inst);

        let surface =
            Self::create_surface(entry, inst, &wl_loader, pdev, &wl_display, &wl_surface).unwrap();

        let ret = Self {
            wl_loader: wl_loader,
            wl_display: wl_display,
            wl_surface: wl_surface,
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
                    format: vk::Format::B8G8R8A8_UNORM,
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
    unsafe fn create_surface(
        entry: &Entry,   // entry and inst aren't used but still need
        inst: &Instance, // to be passed for compatibility
        loader: &khr::WaylandSurface,
        pdev: vk::PhysicalDevice,
        wl_display: &wc::Display,
        wl_surface: &wc::protocol::wl_surface::WlSurface,
    ) -> Result<vk::SurfaceKHR, vk::Result> {
        use std::ops::Deref;
        // TODO: check that the queue we are using supports wayland
        //if !loader.get_physical_device_wayland_presentation_support(pdev, ) {
        //    return Err();
        //}

        // First we need to get our raw C pointers to the wayland objects
        // for &wc::Display, we deref twice to proc the Deref trait to get
        // to a WlDisplay
        let display_ptr = (**wl_display).c_ptr() as *mut _;
        let surface_ptr = wl_surface.as_ref().c_ptr() as *mut _;

        // Now we can collect our info about the wayland surface
        let info = vk::WaylandSurfaceCreateInfoKHR::builder()
            .display(display_ptr)
            .surface(surface_ptr)
            .build();

        // create it
        Ok(loader.create_wayland_surface(&info, None)?)
    }

    /// The two most important extensions are Surface and Wl.
    fn extension_names(_surf_type: &SurfaceType) -> Vec<*const i8> {
        vec![
            khr::Surface::name().as_ptr(),
            khr::WaylandSurface::name().as_ptr(),
            DebugReport::name().as_ptr(),
        ]
    }

    fn get_dpi(&self) -> f32 {
        (0.0, 0.0)
    }
}
