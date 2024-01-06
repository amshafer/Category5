// Release info for dropping WlBuffers when the wm is done with them
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::protocol::wl_buffer;

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
