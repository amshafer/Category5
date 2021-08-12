// The Display object owned by Renderer
//
// Austin Shafer - 2020

#![allow(dead_code, non_camel_case_types)]
extern crate ash;

#[cfg(feature = "macos")]
use ash::extensions::ext::MetalSurface;

#[cfg(feature = "macos")]
extern crate raw_window_handle;
#[cfg(feature = "macos")]
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
#[cfg(feature = "macos")]
extern crate raw_window_metal;
#[cfg(feature = "macos")]
use raw_window_metal::{macos, Layer};

#[cfg(any(feature = "xcb", feature = "macos"))]
extern crate winit;

#[cfg(feature = "wayland")]
extern crate wayland_client as wc;

use ash::extensions::ext::DebugReport;
use ash::extensions::khr;
use ash::vk;
use ash::{Entry, Instance};

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
    #[cfg(feature = "xcb")]
    XcbDisplay(XcbDisplay),
    #[cfg(feature = "macos")]
    MacOSDisplay(MacOSDisplay),
    #[cfg(feature = "wayland")]
    WaylandDisplay(WlDisplay),
}

enum BackendType {
    PhysicalDisplay,
    #[cfg(feature = "xcb")]
    XcbDisplay,
    #[cfg(feature = "macos")]
    MacOSDisplay,
    #[cfg(feature = "wayland")]
    WaylandDisplay,
}

impl Display {
    fn choose_display_backend(info: &CreateInfo) -> BackendType {
        match info.surface_type {
            SurfaceType::Display(_) => BackendType::PhysicalDisplay,
            #[cfg(feature = "xcb")]
            SurfaceType::Xcb(_) => BackendType::XcbDisplay,
            #[cfg(feature = "macos")]
            SurfaceType::MacOS(_) => BackendType::MacOSDisplay,
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
                let n = PhysicalDisplay::new(entry, inst, pdev);
                (Backend::PhysicalDisplay(n.0), n.1, n.2)
            }
            #[cfg(feature = "xcb")]
            SurfaceType::Xcb(win) => {
                let (xd, surf) = XcbDisplay::new(entry, inst, pdev, &win);
                let caps = s_loader
                    .get_physical_device_surface_capabilities(pdev, surf)
                    .unwrap();
                (Backend::XcbDisplay(xd), surf, caps.current_extent)
            }
            #[cfg(feature = "macos")]
            SurfaceType::MacOS(win) => {
                let (xd, surf) = MacOSDisplay::new(entry, inst, pdev, &win);
                let caps = s_loader
                    .get_physical_device_surface_capabilities(pdev, surf)
                    .unwrap();
                (Backend::MacOSDisplay(xd), surf, caps.current_extent)
            }
            #[cfg(feature = "wayland")]
            SurfaceType::Wayland(display, surface) => {
                let (wd, surf) =
                    WlDisplay::new(entry, inst, pdev, display.clone(), surface.clone());
                let caps = s_loader
                    .get_physical_device_surface_capabilities(pdev, surf)
                    .unwrap();
                (Backend::WaylandDisplay(wd), surf, caps.current_extent)
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
            #[cfg(feature = "xcb")]
            Backend::XcbDisplay(xd) => {
                xd.select_surface_format(&self.d_surface_loader, self.d_surface, pdev)
            }
            #[cfg(feature = "macos")]
            Backend::MacOSDisplay(md) => {
                md.select_surface_format(&self.d_surface_loader, self.d_surface, pdev)
            }
            #[cfg(feature = "wayland")]
            Backend::WaylandDisplay(wd) => {
                wd.select_surface_format(&self.d_surface_loader, self.d_surface, pdev)
            }
        }
    }

    pub fn extension_names(info: &CreateInfo) -> Vec<*const i8> {
        match Self::choose_display_backend(info) {
            BackendType::PhysicalDisplay => PhysicalDisplay::extension_names(),
            #[cfg(feature = "xcb")]
            BackendType::XcbDisplay => XcbDisplay::extension_names(),
            #[cfg(feature = "macos")]
            BackendType::MacOSDisplay => MacOSDisplay::extension_names(),
            #[cfg(feature = "wayland")]
            BackendType::WaylandDisplay => WlDisplay::extension_names(),
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
    unsafe fn new(
        entry: &Entry,
        inst: &Instance,
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
    unsafe fn create_surface(
        _entry: &Entry,   // entry and inst aren't used but still need
        _inst: &Instance, // to be passed for compatibility
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

#[cfg(feature = "xcb")]
struct XcbDisplay {
    // the display itself
    pub xcb_loader: khr::XcbSurface,
}

#[cfg(feature = "xcb")]
impl XcbDisplay {
    /// Create an on-screen surface.
    ///
    /// This will grab the function pointer loaders for the
    /// surface and display extensions and then create a
    /// surface to be rendered to.
    unsafe fn new(
        entry: &Entry,
        inst: &Instance,
        pdev: vk::PhysicalDevice,
        win: &winit::window::Window,
    ) -> (Self, vk::SurfaceKHR) {
        let x_loader = khr::XcbSurface::new(entry, inst);

        let surface = Self::create_surface(entry, inst, &x_loader, pdev, win).unwrap();

        let ret = Self {
            xcb_loader: x_loader,
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
    unsafe fn create_surface(
        entry: &Entry,   // entry and inst aren't used but still need
        inst: &Instance, // to be passed for compatibility
        loader: &khr::XcbSurface,
        pdev: vk::PhysicalDevice,
        win: &winit::window::Window,
    ) -> Result<vk::SurfaceKHR, vk::Result> {
        use winit::platform::unix::WindowExtUnix;
        let x11_conn = win.xcb_connection().unwrap();
        let x11_window = win.xlib_window().unwrap();
        let x11_create_info = vk::XcbSurfaceCreateInfoKHR::builder()
            .window(x11_window as u32)
            .connection(x11_conn)
            .build();

        let xcb_surface_loader = khr::XcbSurface::new(entry, inst);
        loader.create_xcb_surface(&x11_create_info, None)
    }

    /// The two most important extensions are Surface and Xcb.
    fn extension_names() -> Vec<*const i8> {
        vec![
            khr::Surface::name().as_ptr(),
            khr::XcbSurface::name().as_ptr(),
            DebugReport::name().as_ptr(),
        ]
    }
}

#[cfg(feature = "macos")]
struct MacOSDisplay {
    // the display itself
    pub mac_loader: MetalSurface,
}

#[cfg(feature = "macos")]
impl MacOSDisplay {
    /// Create an on-screen surface.
    ///
    /// This will grab the function pointer loaders for the
    /// surface and display extensions and then create a
    /// surface to be rendered to.
    unsafe fn new(
        entry: &Entry,
        inst: &Instance,
        pdev: vk::PhysicalDevice,
        win: &winit::window::Window,
    ) -> (Self, vk::SurfaceKHR) {
        let x_loader = MetalSurface::new(entry, inst);

        let surface = Self::create_surface(entry, inst, &x_loader, pdev, win).unwrap();

        let ret = Self {
            mac_loader: x_loader,
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

    unsafe fn create_surface(
        entry: &Entry,   // entry and inst aren't used but still need
        inst: &Instance, // to be passed for compatibility
        loader: &MetalSurface,
        pdev: vk::PhysicalDevice,
        window: &winit::window::Window,
    ) -> Result<vk::SurfaceKHR, vk::Result> {
        // from ash-window/src/lib.rs
        let handle = match window.raw_window_handle() {
            RawWindowHandle::MacOS(handle) => handle,
            _ => panic!("winit raw_window_handle is not of macos type"),
        };

        let layer = match macos::metal_layer_from_handle(handle) {
            Layer::Existing(layer) | Layer::Allocated(layer) => layer as *mut _,
            Layer::None => panic!("No layer was found for macos"),
        };

        let create_info = vk::MetalSurfaceCreateInfoEXT::builder()
            .layer(&*layer)
            .build();

        let metal_surface_loader = MetalSurface::new(entry, inst);
        metal_surface_loader.create_metal_surface(&create_info, None)
    }

    /// The two most important extensions are Surface and Xcb.
    fn extension_names() -> Vec<*const i8> {
        vec![
            khr::Surface::name().as_ptr(),
            MetalSurface::name().as_ptr(),
            DebugReport::name().as_ptr(),
        ]
    }
}

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
    fn extension_names() -> Vec<*const i8> {
        vec![
            khr::Surface::name().as_ptr(),
            khr::WaylandSurface::name().as_ptr(),
            DebugReport::name().as_ptr(),
        ]
    }
}
