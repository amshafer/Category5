/// The platform abstraction
///
/// This hides away the window system code from the rest of Dakota
use crate::dom;
use crate::{
    event::{GlobalEventSystem, OutputEventSystem, PlatformEventSystem},
    OutputId, Result,
};
use std::os::fd::RawFd;

#[cfg(any(feature = "direct2display", feature = "drm"))]
mod display;
#[cfg(any(feature = "direct2display", feature = "drm"))]
pub use display::LibinputPlat;

#[cfg(feature = "sdl")]
mod sdl2;
#[cfg(feature = "sdl")]
pub use self::sdl2::SDL2Plat;

mod headless;
pub use self::headless::HeadlessPlat;

/// Identifies what output type this backend supports
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum BackendType {
    #[cfg(feature = "drm")]
    Drm,
    VkD2d,
}

/// A Dakota platform
///
/// This isolates all of the Window system code. There are a few
/// different backends extracted away by this which handle things like
/// changing the window resolution. The `run` implementation here will
/// run the event loop of this platform, including waiting on any fds
/// added to our watch list.
///
/// This holds the global platform state, individual windows will have
/// their winsys objects held in a separate `OutputPlatform`.
pub trait Platform {
    /// Get the thundr surface type that this platform should use.
    ///
    /// This is where we share our window system object pointers that
    /// Thundr will consume when it creates a `Dispaly` that draws to
    /// this output.
    fn get_th_surf_type<'a>(&self) -> Result<th::SurfaceType>;

    /// Create a window
    ///
    /// This creates a new window output with our winsys, we can
    /// then use this with a Thundr `Display`.
    fn create_output(
        &mut self,
        id: OutputId,
        virtual_output_id: OutputId,
    ) -> Result<Box<dyn OutputPlatform>>;

    /// Create a new virtual window
    ///
    /// This may fail if the platform only supports one virtual surface
    fn create_virtual_output(&mut self, output_ecs: &ll::Instance) -> Result<OutputId>;

    /// Add a watch descriptor to our list. This will cause the platform's
    /// event loop to wake when this fd is readable and queue the UserFd
    /// event.
    fn add_watch_fd(&mut self, fd: RawFd);

    /// Run the event loop for this platform
    ///
    /// This will dispatch winsys handling and will wait for user
    /// input.
    ///
    /// Returns true if we should redraw the app due to an out of
    /// date swapchain.
    fn run(
        &mut self,
        global_evsys: &mut GlobalEventSystem,
        output_queues: &mut ll::Component<OutputEventSystem>,
        platform_queues: &mut ll::Component<PlatformEventSystem>,
        timeout: Option<usize>,
    ) -> Result<()>;
}

/// Platform code for a single window
///
/// This holds the winsys objects for the creation of a single window
/// output. This may be a toplevel window or may be a subsurface.
pub trait OutputPlatform {
    /// Get the thundr surface type that this platform should use.
    ///
    /// This is where we share our window system object pointers that
    /// Thundr will consume when it creates a `Dispaly` that draws to
    /// this output.
    fn get_th_window_info<'a>(&self) -> Result<th::WindowInfo>;

    /// Set the dimensions of this window
    fn set_geometry(&mut self, win: &dom::Window, dims: (u32, u32)) -> Result<()>;
}
