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
use platform::Platform;
pub mod xml;

pub mod event;
pub use event::{AxisSource, GlobalEvent, OutputEvent, PlatformEvent, RawKeycode};
use event::{GlobalEventSystem, OutputEventSystem, PlatformEventSystem};
mod layout;
mod output;
mod virtual_output;
pub use virtual_output::VirtualOutput;
mod render;
pub use output::{Output, OutputInfo};
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

/// Internal type for Output and VirtualOutput resources
///
/// This is our OutputId list. This will be used by other components to
/// access all outputs at once, for example when doing event handling.
pub(crate) type OutputId = DakotaId;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum DakotaObjectType {
    Element,
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
    /// The list of OutputInfos available for use. These can be used to
    /// specify a particular display region while creating an Output.
    d_output_infos: Vec<OutputInfo>,
    /// The current window system backend.
    ///
    /// This may be SDL2 for windowed systems, or direct2display. This handles platform-specific
    /// initialization.
    d_plat: Box<dyn Platform>,
    /// Global event queue
    d_global_event_system: GlobalEventSystem,
    /// Output Id system
    d_output_ecs: ll::Instance,
    /// per-Output event queues
    d_output_event_system: ll::Component<OutputEventSystem>,
    /// per-VirtualOutput event queues
    d_platform_event_system: ll::Component<PlatformEventSystem>,
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
    fn init_thundr(plat: Box<dyn Platform>) -> Result<(Box<dyn Platform>, th::Thundr)> {
        let info = th::CreateInfo::builder()
            .surface_type(plat.get_th_surf_type()?)
            .build();

        let thundr = th::Thundr::new(&info).context("Failed to initialize Thundr")?;

        Ok((plat, thundr))
    }

    /// Create an SDL2 backend
    #[cfg(feature = "sdl")]
    fn create_sdl_platform() -> Result<(Box<dyn Platform>, th::Thundr)> {
        let plat = Box::new(platform::SDL2Plat::new().map_err(|e| {
            log::error!("Failed to create new SDL platform: {:?}", e);
            e
        })?);

        Self::init_thundr(plat)
    }

    /// Create an atomic DRM-KMS backend
    #[cfg(feature = "drm")]
    fn create_drm_platform() -> Result<(Box<dyn Platform>, th::Thundr)> {
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
    fn create_vkd2d_platform() -> Result<(Box<dyn Platform>, th::Thundr)> {
        let plat = Box::new(
            platform::LibinputPlat::new(platform::BackendType::VkD2d).map_err(|e| {
                log::error!("Failed to create new libinput platform: {:?}", e);
                e
            })?,
        );

        Self::init_thundr(plat)
    }

    /// Create a headless platform
    fn create_headless_platform() -> Result<(Box<dyn Platform>, th::Thundr)> {
        let plat = Box::new(platform::HeadlessPlat::new());

        Self::init_thundr(plat)
    }

    /// Try initializing the different plaform backends until we find one that works
    ///
    /// This will test for platform support and initialize the platform, Thundr, and
    /// get the DPI of the display. These three are tested since they all may fail
    /// given different configurations. DPI fails if SDL2 tries to initialize us on
    /// a physical display.
    fn initialize_platform() -> Result<(Box<dyn Platform>, th::Thundr)> {
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
    pub fn new() -> Result<Self> {
        let (plat, thundr) = Self::initialize_platform()?;
        let info = th::CreateInfo::builder()
            .surface_type(plat.get_th_surf_type()?)
            .build();

        let mut output_ecs = ll::Instance::new();
        let output_evsys = output_ecs.add_component();

        let mut output_infos = Vec::with_capacity(1);
        let display_infos = thundr.get_display_info_list(&info)?;
        for info in display_infos {
            output_infos.push(OutputInfo::new(output_evsys.clone(), info));
        }

        Ok(Self {
            d_plat: plat,
            d_output_infos: output_infos,
            d_thund: thundr,
            d_global_event_system: GlobalEventSystem::new(),
            d_output_event_system: output_evsys,
            d_platform_event_system: output_ecs.add_component(),
            d_output_ecs: output_ecs,
        })
    }

    /// Create a new VirtualOutput
    ///
    /// VirtualOutputs represent a theoretical surface that a Scene may be
    /// configured on. The Scene will have layout calculated based on this
    /// VirtualOutput and then some or all of that Scene will be displayed
    /// by a real Output.
    pub fn create_virtual_output(&mut self) -> Option<VirtualOutput> {
        VirtualOutput::new(
            self.d_plat
                .create_virtual_output(&self.d_output_ecs)
                .map_err(|e| {
                    log::error!("Could not create VirtualOutput: {:?}", e);
                    e
                })
                .ok()?,
            self.d_platform_event_system.clone(),
        )
        .ok()
    }

    /// Get the DRM format modifiers supported by Dakota's primary GPU
    pub fn get_supported_drm_render_modifiers(&self) -> Vec<u64> {
        self.d_thund
            .get_primary_dev()
            .get_supported_drm_render_modifiers()
            .iter()
            .map(|m| m.drm_format_modifier)
            .collect()
    }

    /// Get list of OutputInfos
    ///
    /// This returns a list of OutputInfo structures that can be used to create
    /// Outputs. Each OutputInfo represents an abstract presentation region. Multiple
    /// Outputs can possibly be created from an OutputInfo, or only a one to one
    /// relationship may be supported.
    pub fn get_output_info(&self) -> Vec<OutputInfo> {
        self.d_output_infos.clone()
    }

    /// Create a scene compatible with this VirtualOutput
    ///
    /// Resources will be created on the primary, default GPU.
    pub fn create_scene(&self, virtual_output: &VirtualOutput) -> Result<Scene> {
        Scene::new(self.d_thund.get_primary_dev(), virtual_output.get_size())
    }

    /// Create a new Output
    ///
    /// Outputs represent a displayable surface and allow for performing rendering and
    /// presentation. The new Output will be created to be compatible with the provided
    /// VirtualOutput.
    ///
    /// This chooses the default output type. For fine-grained control use
    /// `create_output_with_info`.
    pub fn create_output(&mut self, virtual_output: &VirtualOutput) -> Result<Output> {
        let output_info = self.d_output_infos[0].clone();
        self.create_output_with_info(&output_info, virtual_output)
    }

    /// Create a new Output
    ///
    /// Outputs represent a displayable surface and allow for performing rendering and
    /// presentation. The new Output will be created to be compatible with the provided
    /// VirtualOutput.
    ///
    /// This accepts an OutputInfo parameter, specifying the particular output type to
    /// create.
    pub fn create_output_with_info(
        &mut self,
        output_info: &OutputInfo,
        virtual_output: &VirtualOutput,
    ) -> Result<Output> {
        if !output_info.can_create_output() {
            return Err(anyhow!(
                "Maximum number of Outputs for this OutputInfo has been reached"
            ));
        }

        let output_id = self.d_output_ecs.add_entity();
        let win = self
            .d_plat
            .create_output(output_id.clone(), virtual_output.d_id.clone())
            .map_err(|e| {
                log::error!("Failed to initialize atomic DRM-KMS output: {:?}", e);
                e
            })?;

        let info = th::CreateInfo::builder()
            .surface_type(self.d_plat.get_th_surf_type()?)
            // This is the private information Dakota's platform provides
            .window_info(win.get_th_window_info()?)
            // This is the private information about the virtual/physical
            // output provided by Thundr
            .display_info(output_info.oi_payload.clone())
            .build();

        let display = self
            .d_thund
            .get_display(&info)
            .context("Failed to get Thundr Display")?;

        let ret = Output::new(win, display, output_id, self.d_output_event_system.clone());
        // If we successfully created an Output, add its id to our OutputInfo for tracking
        if let Ok(output) = &ret {
            output_info.add_output(output.d_id.clone());
        }

        return ret;
    }

    /// Add a file descriptor to watch
    ///
    /// This will add a new file descriptor to the watch set inside dakota,
    /// meaning dakota will return control to the user when this fd is readable.
    /// This is done through the `UserFdReadable` event.
    pub fn add_watch_fd(&mut self, fd: RawFd) {
        self.d_plat.add_watch_fd(fd);
    }

    /// Drain the queue of currently unhandled events
    ///
    /// The app should do this in its main loop after dispatching.
    /// These will be cleared during each dispatch.
    pub fn drain_events<'a>(&'a mut self) -> std::collections::vec_deque::Drain<'a, GlobalEvent> {
        self.d_global_event_system.drain_events()
    }

    /// run the main Dakota platform loop
    ///
    /// This waits for incoming events which will trigger user input or rendering
    /// to take place.
    pub fn dispatch(&mut self, timeout: Option<usize>) -> Result<()> {
        self.d_plat.run(
            &mut self.d_global_event_system,
            &mut self.d_output_event_system,
            &mut self.d_platform_event_system,
            timeout,
        )
    }
}
