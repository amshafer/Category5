/// The platform abstraction
///
/// This hides away the window system code from the rest of Dakota
use crate::dom;
use crate::dom::DakotaDOM;
use crate::{event::EventSystem, DakotaError, Result};
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

/// A Dakota platform
///
/// This isolates all of the Window system code.
pub trait Platform {
    fn get_th_surf_type<'a>(&self) -> Result<th::SurfaceType>;

    fn set_output_params(&mut self, win: &dom::Window, dims: (u32, u32)) -> Result<()>;

    fn add_watch_fd(&mut self, fd: RawFd);

    /// Returns true if we should redraw the app
    fn run(
        &mut self,
        evsys: &mut EventSystem,
        dom: &DakotaDOM,
        timeout: Option<usize>,
    ) -> std::result::Result<bool, DakotaError>;
}
