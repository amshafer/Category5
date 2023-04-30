// Release info for dropping WlBuffers when the wm is done with them
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::protocol::wl_buffer;

use std::fmt;
#[cfg(debug_assertions)]
use std::os::unix::io::AsRawFd;
use std::os::unix::io::OwnedFd;
use utils::log;

/// Dmabuf release info
///
/// Should be paired with a Dmabuf while it is being
/// imported. Once the import is complete AND it is
/// replaced by the next commit, the dmabuf's wl_buffer
/// should be released so the client can reuse it.
pub struct DmabufReleaseInfo {
    // the drm fd for debugging purposes
    pub dr_fd: OwnedFd,
    // The wl_buffer that represents this dmabuf
    pub dr_wl_buffer: wl_buffer::WlBuffer,
}

impl DmabufReleaseInfo {
    pub fn release(&mut self) {
        log::debug!("Releasing wl_buffer for dmabuf {}", self.dr_fd.as_raw_fd());
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

pub struct GenericReleaseInfo {
    // The wl_buffer that represents this  image
    pub wl_buffer: wl_buffer::WlBuffer,
}

impl GenericReleaseInfo {
    pub fn release(&mut self) {
        self.wl_buffer.release();
    }
}

impl Drop for GenericReleaseInfo {
    fn drop(&mut self) {
        self.release();
    }
}
