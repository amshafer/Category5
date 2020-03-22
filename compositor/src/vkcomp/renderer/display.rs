// The Display object owned by Renderer
//
// Austin Shafer - 2020

#![allow(dead_code, non_camel_case_types)]
extern crate ash;

use ash::version::{EntryV1_0, InstanceV1_0};
use ash::vk;
use ash::extensions::khr;
use ash::extensions::ext::DebugReport;

use std::ffi::{CStr};
// A display represents a physical screen
//
// This is mostly the same as vulkan's concept of a display,
// but it is a bit different. This name is overloaded as vulkan,
// ash, and us have something called a display. Essentially
// this holds the PFN loaders, the display KHR extension object,
// and the surface generated for the physical display.
//
// The swapchain is generated (and regenerated) from this stuff.
pub struct Display {
    // the actual surface (KHR extension)
    pub surface: vk::SurfaceKHR,
    // the display itself
    pub display: vk::DisplayKHR,
    // The mode the display was created with
    pub display_mode: vk::DisplayModeKHR,
    // function pointer loaders
    pub surface_loader: khr::Surface,
    pub display_loader: khr::Display,
    pub resolution: vk::Extent2D,
}

impl Display {
    // Create an on-screen surface.
    //
    // This will grab the function pointer loaders for the
    // surface and display extensions and then create a
    // surface to be rendered to.
    pub unsafe fn new<E: EntryV1_0, I: InstanceV1_0>
        (entry: &E,
         inst: &I,
         pdev: vk::PhysicalDevice)
        -> Display
    {
        let d_loader = khr::Display::new(entry, inst);
        let s_loader = khr::Surface::new(entry, inst);

        let (display, surface, mode, resolution) =
            Display::create_surface(entry, inst, &d_loader, pdev)
            .unwrap();

        Display {
            surface_loader: s_loader,
            display_loader: d_loader,
            display_mode: mode,
            display: display,
            surface: surface,
            resolution: resolution,
        }
    }

    // Selects a resolution for the renderer
    //
    // We saved the resolution of the display surface when we created
    // it. If the surface capabilities doe not specify a requested
    // extent, then we will return the screen's resolution.
    pub unsafe fn select_resolution(&self,
                                surface_caps: &vk::SurfaceCapabilitiesKHR)
                                -> vk::Extent2D
    {
        match surface_caps.current_extent.width {
            std::u32::MAX => self.resolution,
            _ => surface_caps.current_extent,
        }
    }

    // choose a vkSurfaceFormatKHR for the vkSurfaceKHR
    //
    // This selects the color space and layout for a surface. This should
    // be called by the Renderer after creating a Display.
    pub unsafe fn select_surface_format(&self,
                                        pdev: vk::PhysicalDevice)
                                        -> vk::SurfaceFormatKHR
    {
        let formats = self.surface_loader
            .get_physical_device_surface_formats(pdev, self.surface)
            .unwrap();

        formats.iter()
            .map(|fmt| match fmt.format {
                // if the surface does not specify a desired format
                // then we can choose our own
                vk::Format::UNDEFINED => vk::SurfaceFormatKHR {
                    format: vk::Format::B8G8R8_UNORM,
                    color_space: fmt.color_space,
                },
                // if the surface has a desired format we will just
                // use that
                _ => *fmt,
            })
            .nth(0)
            .expect("Could not find a surface format")
    }


    // Get a physical display surface.
    //
    // This returns the surfaceKHR to create a swapchain with, the
    // mode the display is using, and the resolution of the screen.
    // The resolution is returned here to avoid having to recall the
    // vkGetDisplayModeProperties function a second time.
    //
    // Yea this has a gross amount of return values...
    #[cfg(unix)]
    unsafe fn create_surface<E: EntryV1_0, I: InstanceV1_0>
        (_entry: &E, // entry and inst aren't used but still need
         _inst: &I, // to be passed for compatibility
         loader: &khr::Display,
         pdev: vk::PhysicalDevice)
         -> Result<(vk::DisplayKHR,
                    vk::SurfaceKHR,
                    vk::DisplayModeKHR,
                    vk::Extent2D),
                   vk::Result>
    {
        // This is essentially a list of the available displays.
        // Despite having a display_name member, the names are very
        // unhelpful. (e.x. "monitor").
        let disp_props = loader
            .get_physical_device_display_properties(pdev)
            .unwrap();

        for (i,p) in disp_props.iter().enumerate() {
            println!("{} display: {:?}", i, CStr::from_ptr(p.display_name));
        }

        // The available modes for the display. This holds the resolution.
        let mode_props = loader
            .get_display_mode_properties(pdev,
                                         disp_props[0].display)
            .unwrap();

        for (i,m) in mode_props.iter().enumerate() {
            println!("display 0 - {} mode: {:?}", i,
                     m.parameters.refresh_rate);
        }

        // As of now we are not doing anything important with planes,
        // but it is still useful to see which ones are reported by
        // the hardware.
        let plane_props = loader
            .get_physical_device_display_plane_properties(pdev)
            .unwrap();

        for (i,p) in plane_props.iter().enumerate() {
            println!("display 0 - plane: {} at stack {}", i,
                     p.current_stack_index);

            let supported = loader
                .get_display_plane_supported_displays(pdev,
                                                      0) // plane index
                .unwrap();

            for (i,d) in disp_props.iter().enumerate() {
                if supported.contains(&d.display) {
                    println!("  supports display {}", i);
                }
            }
        }

        // create a display mode from the parameters we got earlier
        let mode_info = vk::DisplayModeCreateInfoKHR::builder()
            .parameters(mode_props[0].parameters);
        let mode = loader
            .create_display_mode(pdev,
                                 disp_props[0].display,
                                 &mode_info,
                                 None)
            .unwrap();

        // Print out the plane capabilities
        for (i,_) in plane_props.iter().enumerate() {
            let caps = loader.get_display_plane_capabilities(
                pdev,
                mode,
                i as u32,
            ).unwrap();
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
                mode_props[0].parameters.visible_region
            )),
            Err(e) => Err(e),
        }
    }

    // this should really go in its own Platform module
    //
    // The two most important extensions are Surface and Display.
    // Without them we cannot render anything.
    pub fn extension_names() -> Vec<*const i8> {
        vec![
            khr::Surface::name().as_ptr(),
            khr::Display::name().as_ptr(),
            DebugReport::name().as_ptr(),
        ]
    }

    pub fn destroy (&mut self) {
        println!("Destroying display");
        unsafe {
            self.surface_loader.destroy_surface(
                self.surface,
                None
            );
        }
        // It seems that the display resources (mode) are cleaned up
        // when the surface is destroyed. There are not separate
        // deconstructors for them
        //
        // The validation layers do warn about them however (bug?)
    }
}

