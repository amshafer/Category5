/// Headless Dakoat Platform
///
/// This is used for testing code at the moment
///
/// Austin Shafer - 2024
use super::{OutputPlatform, Platform};
use crate::dom;
use crate::{
    event::{GlobalEventSystem, OutputEventSystem, PlatformEventSystem},
    OutputId, Result,
};
use std::os::fd::RawFd;
use utils::log;

pub struct HeadlessPlat();
pub struct HeadlessOutput();

impl HeadlessPlat {
    pub fn new() -> Self {
        Self()
    }
}

impl OutputPlatform for HeadlessOutput {
    fn get_th_window_info<'a>(&self) -> Result<th::WindowInfo> {
        Ok(th::WindowInfo::Headless)
    }

    /// This doesn't make sense to implement, since the final size is just whatever
    /// the size of the screen is.
    fn set_geometry(&mut self, _win: &dom::Window, _dims: (u32, u32)) -> Result<()> {
        log::debug!("set_output_params on headless is a noop");
        Ok(())
    }
}

impl Platform for HeadlessPlat {
    fn create_output(
        &mut self,
        _id: OutputId,
        _virtual_output_id: OutputId,
    ) -> Result<Box<dyn OutputPlatform>> {
        Ok(Box::new(HeadlessOutput {}))
    }

    /// Create a new virtual window
    ///
    /// This may fail if the platform only supports one virtual surface
    fn create_virtual_output(&mut self) -> bool {
        true
    }

    fn get_th_surf_type<'a>(&self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::Headless)
    }

    fn add_watch_fd(&mut self, _fd: RawFd) {}

    fn run(
        &mut self,
        _global_evsys: &mut GlobalEventSystem,
        _output_queues: &mut ll::Component<OutputEventSystem>,
        _platform_queues: &mut ll::Component<PlatformEventSystem>,
        _timeout: Option<usize>,
    ) -> Result<()> {
        std::thread::sleep(std::time::Duration::from_millis(32));

        Ok(())
    }
}
