///! Dakota UI Toolkit
///!
///! Dakota is a UI toolkit designed for rendering trees of surfaces. These
///! surfaces can be easily expressed in XML documents, and updated dynamically
///! by the application.
///!
// Austin Shafer - 2022
extern crate fontconfig as fc;
extern crate freetype as ft;
extern crate image;
extern crate lluvia as ll;
extern crate thundr as th;
pub use th::ThundrError as DakotaError;
pub use th::{Damage, Dmabuf, DmabufPlane, Droppable, MappedImage};

extern crate bitflags;

extern crate lazy_static;
extern crate utils;
use utils::log;
pub use utils::MemImage;
pub use utils::{
    anyhow, fdwatch::FdWatch, region::Rect, timing::StopWatch, Context, Error, Result,
};

pub mod dom;
pub mod input;
#[cfg(test)]
mod tests;
pub use crate::input::{Keycode, MouseButton};
mod platform;
use platform::{OutputPlatform, Platform};
pub mod xml;

pub mod event;
pub use event::{AxisSource, Event, RawKeycode};
mod layout;
mod output;
mod render;
pub use output::Output;
mod font;
mod scene;
pub use scene::Scene;

use std::os::fd::RawFd;

/// Dakota Object Id
///
/// This is a resource handle which is used to look up information
/// in a variety of Entity-Component tables. This allows us to attach
/// arbitrary state to Dakota Elements, both in Dakota's implementation
/// any in the client applications.
pub type DakotaId = ll::Entity;

/// This is our OutputId list. This will be used by other components to
/// access all outputs at once, for example when doing copies.
///
/// Outputs are kept behind these Ids as it provides a convenient way to
/// identify them in events. For example, when a presentation event takes
/// place the OutputId of the presented display will be included.
pub type OutputId = DakotaId;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DakotaObjectType {
    Element,
    DakotaDOM,
    Font,
}

/// Only one of content or children may be defined,
/// they are mutually exclusive.
///
/// Element layout will:
///   a) expand horizontally to fit their container
///   b) expand vertically to fit their container
///   c) a element's content is scaled to fit the element.
///   d) default behavior is only vertical scrolling allowed for
///      when the element's content is longer than the element's height.
///      d.1) if the user does not specify a vertical/horizontal scrolling,
///           then that edge of the element is static. It is basically
///           a window, and scrolling may occur within that element in
///           whatever dimensions were not marked as scrolling.
///           (e.g. default behavior is a horizontal scrolling = false
///            and vertical scrolling = true)
///   e) a-b may be limited by dimensions specified by the user.
///      the dimensions are not specified, then the resource's
///      default size is used.
///   f) regarding (e), if the element's size does not fill the container,
///      then:
///      f.1) the elementes will be laid out horizontally first,
///      f.2) with vertical wrapping if there is not enough room.
pub struct Dakota {
    // GROSS: we need thund to be before plat so that it gets dropped first
    // It might reference the window inside plat, and will segfault if
    // dropped after it.
    d_thund: th::Thundr,
    /// The current window system backend.
    ///
    /// This may be SDL2 for windowed systems, or direct2display. This handles platform-specific
    /// initialization.
    d_plat: Box<dyn Platform>,
    /// Output Id system
    d_output_ecs: ll::Instance,
}

/// Enum for specifying subsurface operations
pub enum SubsurfaceOrder {
    Above,
    Below,
}

impl Dakota {
    /// Helper for initializing Thundr for a given platform.
    ///
    /// Here we create an output platform that we can then initialize thundr
    /// from. Because this is the first window we need to provide a surface type
    /// so thundr knows what Vulkan extensions to enable.
    fn init_thundr(mut plat: Box<dyn Platform>) -> Result<(Box<dyn Platform>, th::Thundr)> {
        let info = th::CreateInfo::builder()
            .surface_type(win.get_th_surf_type()?)
            .build();

        let mut thundr = th::Thundr::new(&info).context("Failed to initialize Thundr")?;

        Ok((plat, thundr))
    }

    /// Create an SDL2 backend
    #[cfg(feature = "sdl")]
    fn create_sdl_platform() -> Result<(
        Box<dyn Platform>,
        Box<dyn OutputPlatform>,
        th::Thundr,
        th::Display,
    )> {
        let plat = Box::new(platform::SDL2Plat::new().map_err(|e| {
            log::error!("Failed to create new SDL platform: {:?}", e);
            e
        })?);

        Self::init_thundr(plat)
    }

    /// Create an atomic DRM-KMS backend
    #[cfg(feature = "drm")]
    fn create_drm_platform() -> Result<(
        Box<dyn Platform>,
        Box<dyn OutputPlatform>,
        th::Thundr,
        th::Display,
    )> {
        let plat = Box::new(
            platform::LibinputPlat::new(platform::BackendType::Drm).map_err(|e| {
                log::error!("Failed to create new libinput platform: {:?}", e);
                e
            })?,
        );

        Self::init_thundr(plat)
    }

    /// Create a Vulkan "Direct to Display" platform
    #[cfg(feature = "direct2display")]
    fn create_vkd2d_platform() -> Result<(
        Box<dyn Platform>,
        Box<dyn OutputPlatform>,
        th::Thundr,
        th::Display,
    )> {
        let plat = Box::new(
            platform::LibinputPlat::new(platform::BackendType::VkD2d).map_err(|e| {
                log::error!("Failed to create new libinput platform: {:?}", e);
                e
            })?,
        );

        Self::init_thundr(plat)
    }

    /// Create a headless platform
    fn create_headless_platform() -> Result<(
        Box<dyn Platform>,
        Box<dyn OutputPlatform>,
        th::Thundr,
        th::Display,
    )> {
        let plat = Box::new(platform::HeadlessPlat::new());

        Self::init_thundr(plat)
    }

    /// Try initializing the different plaform backends until we find one that works
    ///
    /// This will test for platform support and initialize the platform, Thundr, and
    /// get the DPI of the display. These three are tested since they all may fail
    /// given different configurations. DPI fails if SDL2 tries to initialize us on
    /// a physical display.
    fn initialize_platform() -> Result<(
        Box<dyn Platform>,
        Box<dyn OutputPlatform>,
        th::Thundr,
        th::Display,
    )> {
        if std::env::var("DAKOTA_HEADLESS_BACKEND").is_err() {
            // ------------------------------------------------------------------------
            // SDL 2
            // ------------------------------------------------------------------------
            // If we are not forcing headless mode, start by attempting sdl
            #[cfg(feature = "sdl")]
            if std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok() {
                if let Ok(ret) = Self::create_sdl_platform() {
                    log::debug!("Using SDL2");
                    return Ok(ret);
                }
            }

            // ------------------------------------------------------------------------
            // DRM
            // ------------------------------------------------------------------------
            #[cfg(feature = "drm")]
            if let Ok(ret) = Self::create_drm_platform() {
                log::debug!("Using Atomic DRM-KMS");
                return Ok(ret);
            }

            // ------------------------------------------------------------------------
            // Vulkan Direct to Display
            // ------------------------------------------------------------------------
            #[cfg(feature = "direct2display")]
            if let Ok(ret) = Self::create_vkd2d_platform() {
                log::debug!("Using Vulkan Direct to Display");
                return Ok(ret);
            }
        }

        // ------------------------------------------------------------------------
        // Headless
        // ------------------------------------------------------------------------
        if let Ok(ret) = Self::create_headless_platform() {
            log::debug!("Using Vulkan Direct to Display");
            return Ok(ret);
        }

        return Err(anyhow!("Could not find available platform"));
    }

    /// Construct a new Dakota instance
    ///
    /// This will initialize the window system platform layer, create a thundr
    /// instance from it, and wrap it in Dakota.
    ///
    /// This returns the main Dakota instance along with the primary/default
    /// output.
    pub fn new() -> Result<(Self, Output)> {
        let (plat, thundr) = Self::initialize_platform()?;
        let primary_output = Output::new(window_plat, display)?;

        let output_ecs = ll::Instance::new();

        Ok((
            Self {
                d_plat: plat,
                d_thund: thundr,
                d_output_ecs: output_ecs,
            },
            primary_output,
        ))
    }

    /// Create a new Output
    ///
    /// Outputs represent a displayable surface and allow for performing
    /// rendering and presentation.
    pub fn create_output(&mut self) -> Result<Output> {
        let win = plat.create_output().map_err(|e| {
            log::error!("Failed to initialize atomic DRM-KMS output: {:?}", e);
            e
        })?;

        let info = th::CreateInfo::builder()
            .surface_type(win.get_th_surf_type()?)
            .window_info(win.get_th_window_info()?)
            .build();

        let display = thundr
            .get_display(&info)
            .context("Failed to get Thundr Display")?;

        Output::new(window_plat, display)
    }

    /// Add a file descriptor to watch
    ///
    /// This will add a new file descriptor to the watch set inside dakota,
    /// meaning dakota will return control to the user when this fd is readable.
    /// This is done through the `UserFdReadable` event.
    pub fn add_watch_fd(&mut self, fd: RawFd) {
        self.d_plat.add_watch_fd(fd);
    }
}
