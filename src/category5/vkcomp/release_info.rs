// Release info for dropping WlBuffers when the wm is done with them
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::protocol::wl_buffer;

use std::fmt;
use std::os::unix::io::RawFd;
use utils::log;

/// Dmabuf release info
///
/// Should be paired with a Dmabuf while it is being
/// imported. Once the import is complete AND it is
/// replaced by the next commit, the dmabuf's wl_buffer
/// should be released so the client can reuse it.
pub struct DmabufReleaseInfo {
    // the drm fd for debugging purposes
    pub dr_fd: RawFd,
    // The wl_buffer that represents this dmabuf
    pub dr_wl_buffer: wl_buffer::WlBuffer,
}

impl DmabufReleaseInfo {
    pub fn release(&mut self) {
        log::profiling!("Releasing wl_buffer for dmabuf {}", self.dr_fd);
        self.dr_wl_buffer.release();
    }
}

impl Drop for DmabufReleaseInfo {
    fn drop(&mut self) {
        self.release();
    }
}

impl fmt::Debug for DmabufReleaseInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DmabufReleaseInfo")
            .field("fd", &self.dr_fd)
            .field("dr_wl_buffer", &"<wl_buffer omitted>".to_string())
            .finish()
    }
}
