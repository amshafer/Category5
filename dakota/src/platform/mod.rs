#![allow(dead_code)]
use crate::dom;
use crate::dom::DakotaDOM;
use crate::{event::EventSystem, DakotaError, Result};
use std::os::fd::RawFd;

#[cfg(feature = "direct2display")]
mod display;
#[cfg(feature = "direct2display")]
pub use display::DisplayPlat;

#[cfg(feature = "sdl")]
mod sdl2;
#[cfg(feature = "sdl")]
pub use self::sdl2::SDL2Plat;

pub trait Platform {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType>;

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
