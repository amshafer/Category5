//! # The Thundr rendering toolkit.
//!
//! Thundr is a Vulkan composition library for use in ui toolkits and
//! wayland compositors. You use it to create a set of images from
//! textures or window contents, attach those images to surfaces, and pass
//! a list of surfaces to thundr for rendering.
//!
//! Thundr also supports multiple methods of drawing:
//! * `geometric` - This is a more "traditional" manner of drawing ui elements:
//! surfaces are drawn as textured quads in 3D space.
//!
//! ## Drawing API
//!
//! The general flow of a thundr client is as follows:
//! * Create an Image (`create_image_from*`)
//!   * Use a MemImage to load a texture from raw bits.
//!   * Use a dmabuf to load a image contents from a gpu buffer.
//! * Create a Surface (`create_surface`)
//!   * Assign it a location and a size
//! * Create a surface list (`SurfaceList::new()`)
//!   * Push the surfaces you'd like rendered into the list from front to
//!   back (`SurfaceList.push`)
//! * Tell Thundr to launch the work on the gpu (`draw_frame`)
//! * Present the rendering results on screen (`present`)
//!
//! ```
//! use thundr as th;
//!
//! // specify surface type here, this example uses headless
//! let info = th::CreateInfo::builder()
//!     .surface_type(th::SurfaceType::Headless)
//!     .build();
//!
//! let mut thund = th::Thundr::new(&info).unwrap();
//! let mut display = thund.get_display(&info).unwrap();
//! let res = display.get_resolution();
//!
//! let pixels: Vec<u8> = std::iter::repeat(128).take(4 * 64 * 64).collect();
//! // Create an image from our MemImage
//! let image = display
//!     .d_dev
//!     .create_image_from_bits(
//!         pixels.as_slice(),
//!         64, // width of texture
//!         64, // height of texture
//!         64, // stride
//!         None,
//!     )
//!     .unwrap();
//!
//! // Begin recording drawing commands
//! let mut frame = display.acquire_next_frame().unwrap();
//!
//! // Set the current drawing viewport. Drawing operations will take place
//! // within this region.
//! let viewport = th::Viewport::new(0, 0, res.0 as i32, res.1 as i32);
//! frame.set_viewport(&viewport).unwrap();
//!
//! // Draw a 16x16 surface at position (0, 0) referencing our image
//! let surf = th::Surface::new(th::Rect::new(0, 0, 16, 16), None);
//! frame.draw_surface(&surf, Some(&image)).unwrap();
//!
//! // present the frame
//! frame.present().unwrap();
//! ```
//! ## Requirements
//!
//! Thundr requires a system with vulkan 1.2+ installed. The following
//! extensions are used:
//! * VK_KHR_surface
//! * VK_KHR_display
//! * VK_EXT_maintenance2
//! * VK_KHR_debug_report
//! * VK_KHR_descriptor_indexing
//! * VK_KHR_external_memory

extern crate lazy_static;
extern crate lluvia;
use lluvia as ll;

// Austin Shafer - 2020
use std::marker::PhantomData;
use std::sync::Arc;

mod damage;
mod deletion_queue;
mod descpool;
mod device;
mod display;
mod image;
mod instance;
mod pipelines;
mod platform;
mod surface;

#[cfg(test)]
mod tests;

#[cfg(feature = "sdl")]
extern crate sdl2;

pub use self::image::Image;
pub use self::image::{Dmabuf, DmabufPlane};
pub use damage::Damage;
pub(crate) use deletion_queue::DeletionQueue;
pub use device::Device;
pub use display::{frame::FrameRenderer, Display};
use instance::Instance;
pub use surface::Surface;

// Re-export some things from utils so clients
// can use them
extern crate utils;
pub use crate::utils::region::Rect;
pub use crate::utils::{anyhow, Context, MemImage};

pub type Result<T> = std::result::Result<T, ThundrError>;

#[cfg(feature = "wayland")]
extern crate wayland_client as wc;

#[macro_use]
extern crate memoffset;

extern crate thiserror;
use thiserror::Error;

/// Thundr error codes
/// These signify that action should be taken by the app.
#[derive(Error, Eq, PartialEq, Debug)]
#[allow(non_camel_case_types)]
pub enum ThundrError {
    #[error("Operation timed out")]
    TIMEOUT,
    #[error("Allocation failure")]
    OUT_OF_MEMORY,
    #[error("Operation is not ready, it needs to be redone")]
    NOT_READY,
    #[error("Failed to acquire the next swapchain image")]
    COULD_NOT_ACQUIRE_NEXT_IMAGE,
    #[error("vkQueuePresent failed")]
    PRESENT_FAILED,
    #[error("The internal Vulkan swapchain is out of date")]
    OUT_OF_DATE,
    #[error("Vulkan surface does not support R8G8B8A8_UNORM")]
    VK_SURF_NOT_SUPPORTED,
    #[error("Vulkan surface does not support the necessary (bindless) extensions")]
    VK_NOT_ALL_EXTENSIONS_AVAILABLE,
    #[error("Please select a composition type in the thundr CreateInfo")]
    COMPOSITION_TYPE_NOT_SPECIFIED,
    #[error("Vulkan surface or subsurface could not be found")]
    SURFACE_NOT_FOUND,
    #[error("Thundr Usage Bug: Recording already in progress")]
    RECORDING_ALREADY_IN_PROGRESS,
    #[error("Thundr Usage Bug: Recording has not been started")]
    RECORDING_NOT_IN_PROGRESS,
    #[error("Invalid Operation")]
    INVALID,
    #[error("Invalid File Descriptor")]
    INVALID_FD,
    #[error("Could not create the Vulkan swapchain")]
    COULD_NOT_CREATE_SWAPCHAIN,
    #[error("Failed to create Vulkan image")]
    COULD_NOT_CREATE_IMAGE,
    #[error("Invalid format or no format found")]
    INVALID_FORMAT,
    #[error("Could not get a valid display backend")]
    NO_DISPLAY,
    #[error("Could not import dmabuf")]
    INVALID_DMABUF,
    #[error("Stride does not match dimensions and size of image data")]
    INVALID_STRIDE,
    #[error("Input error")]
    IOERROR,
}

impl From<std::io::Error> for ThundrError {
    fn from(_val: std::io::Error) -> Self {
        ThundrError::IOERROR
    }
}

pub struct Thundr {
    /// Our primary device
    th_dev: Arc<Device>,
    /// We keep a list of all the images allocated by this context
    /// so that Pipeline::draw doesn't have to dedup the surfacelist's images
    pub th_image_ecs: ll::Instance,
}

/// A region to display to
///
/// The viewport will control what section of the screen is rendered
/// to. You will specify it when performing draw calls.
#[derive(Debug, Clone)]
pub struct Viewport {
    /// This is the position of the viewport on the output
    pub offset: (i32, i32),
    /// Size of the viewport within the output
    pub size: (i32, i32),
    /// The scrolling region of this viewport, basically the maximum bounds
    /// within which it is valid to update `scroll_offset`. This is similar to
    /// the panning region in X11.
    pub scroll_region: (i32, i32),
    /// This is the amount to offset everything within this viewport by. It
    /// can be used to move around all internal elements without updating
    /// them.
    ///
    /// This may be in the [0, scroll_region] range
    pub scroll_offset: (i32, i32),
}

impl Viewport {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            offset: (x, y),
            size: (width, height),
            scroll_region: (width, height),
            scroll_offset: (0, 0),
        }
    }

    /// Update the valid scrolling region within this viewport
    pub fn set_scroll_region(&mut self, x: i32, y: i32) {
        self.scroll_region = (x, y);
    }

    /// Set the scrolling within this viewport. This is a global transform
    ///
    /// This performs bounds checking of `dx` and `dy` to ensure the are within
    /// `scroll_region`. If they are not, then no scrolling is performed.
    pub fn update_scroll_amount(&mut self, dx: i32, dy: i32) {
        // The min and max bounds here are weird. Think of it like moving the
        // scroll region, not moving the scroll area. It looks like this:
        //
        // R: scroll region
        // A: scroll area
        //
        // Here they are at zero, content has just been loaded:
        //              0
        //              R--------------------R
        //              A-------------A
        //
        // Now here they are with the scroll all the way complete:
        //              0
        //       R--------------------R
        //              A-------------A
        //
        // The offset is actually from [-(R - A), 0]
        let min_x = -1 * (self.scroll_region.0 - self.size.0);
        let max_x = 0;
        // now get the new offset
        let x_offset = self.scroll_offset.0 - dx;
        // clamp this offset within our bounds
        let x_clamped = x_offset.clamp(min_x, max_x);

        let min_y = -1 * (self.scroll_region.1 - self.size.1);
        let max_y = 0;
        let y_offset = self.scroll_offset.1 - dy;
        let y_clamped = y_offset.clamp(min_y, max_y);

        self.scroll_offset = (x_clamped, y_clamped);
    }
}

pub enum SurfaceType<'a> {
    Headless,
    #[cfg(feature = "drm")]
    Drm,
    /// it exists to make the lifetime parameter play nice with rust.
    /// Since the Display variant doesn't have a lifetime, we need one that
    /// does incase xcb/macos aren't enabled.
    Display(PhantomData<&'a usize>),
    #[cfg(feature = "sdl")]
    SDL2(&'a sdl2::VideoSubsystem, &'a sdl2::video::Window),
}

/// Parameters for Thundr creation.
///
/// These will be set by Thundr based on the Pipelines that will
/// be enabled. See `Pipeline` for methods that drive the data
/// contained here.
pub struct CreateInfo<'a> {
    pub surface_type: SurfaceType<'a>,
}

impl<'a> CreateInfo<'a> {
    pub fn builder() -> CreateInfoBuilder<'a> {
        CreateInfoBuilder {
            ci: CreateInfo {
                surface_type: SurfaceType::Display(PhantomData),
            },
        }
    }
}

/// Implements the builder pattern for easier thundr creation
pub struct CreateInfoBuilder<'a> {
    ci: CreateInfo<'a>,
}
impl<'a> CreateInfoBuilder<'a> {
    pub fn surface_type(mut self, ty: SurfaceType<'a>) -> Self {
        self.ci.surface_type = ty;
        self
    }

    pub fn build(self) -> CreateInfo<'a> {
        self.ci
    }
}

/// Droppable trait that matches anything.
///
/// From <https://doc.rust-lang.org/rustc/lints/listing/warn-by-default.html#dyn-drop>
///
/// To work around passing dyn Drop we specify a trait that can accept anything. That
/// way this boxed object can be dropped when the last rendering resource references
/// it.
pub trait Droppable {}
impl<T> Droppable for T {}

/// A mapped VkImage
///
/// This is used to expose a CPU mapping of a VkImage. The main use
/// case being for automated testing, we use this to dump the contents
/// of a swapchain image to compare against a correct result.
#[allow(dead_code)]
pub struct MappedImage {
    pub mi_data: Vec<u8>,
}

// This is the public facing thundr api. Don't change it
impl Thundr {
    // TODO: make get_available_params and add customization
    pub fn new(info: &CreateInfo) -> Result<Thundr> {
        // Create our own ECS for the image resources
        let mut img_ecs = ll::Instance::new();

        let inst = Arc::new(Instance::new(&info));
        let dev = Device::new(inst, &mut img_ecs, info)?;

        Ok(Thundr {
            th_dev: dev,
            th_image_ecs: img_ecs,
        })
    }

    /// Get a display object to draw with
    ///
    /// Display objects represent a particular output, either a window in a desktop
    /// system or a physical display. Display abstracts away the swapchain platform
    /// and holds the drawing commands.
    pub fn get_display(&mut self, info: &CreateInfo) -> Result<Display> {
        Display::new(info, self.th_dev.clone())
    }

    /// Update an existing image from a shm buffer
    pub fn update_image_from_bits(
        &mut self,
        image: &Image,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32,
        damage: Option<Damage>,
        release: Option<Box<dyn Droppable + Send + Sync>>,
    ) -> Result<()> {
        self.th_dev
            .update_image_from_bits(image, data, width, height, stride, damage, release)
    }
}
