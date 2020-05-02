// wl_surface interface
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::Main;
use ws::protocol::{wl_buffer, wl_surface as wlsi};

use crate::category5::vkcomp::wm;
use super::shm::*;

use std::sync::mpsc::Sender;


// Private structure for a wayland surface
//
// A surface represents a visible area on screen. Desktop organization
// effects and other transformations are taken care of by a 'shell'
// interface, not this. A surface will have a buffer attached to it which
// will be displayed to the client when it is committed.
#[allow(dead_code)]
pub struct Surface {
    s_id: u64, // The id of the window in the renderer
    // The currently attached buffer. Will be displayed on commit
    // When the window is created a buffer is not assigned, hence the option
    s_attached_buffer: Option<wl_buffer::WlBuffer>,
    // the s_attached_buffer is moved here to signify that we can draw
    // with it.
    pub s_committed_buffer: Option<wl_buffer::WlBuffer>,
    // the location of the surface in our compositor
    s_x: u32,
    s_y: u32,
    s_wm_tx: Sender<wm::task::Task>,
}

impl Surface {
    // Handle a request from a client
    //
    pub fn handle_request(&mut self,
                          surf: Main<wlsi::WlSurface>,
                          req: wlsi::Request)
    {
        match req {
            wlsi::Request::Attach { buffer, x, y } =>
                self.attach(surf, buffer, x, y),
            wlsi::Request::Commit =>
                self.commit(),
            wlsi::Request::Destroy =>
                self.destroy(),
            _ => unimplemented!(),
        }
    }

    // attach a wl_buffer to the surface
    //
    // The client crafts a buffer with care, and tells us that it will be
    // backing the surface represented by `resource`. `buffer` will be
    // placed in the private struct that the compositor made.
    fn attach(&mut self,
              _surf: Main<wlsi::WlSurface>,
              buf: Option<wl_buffer::WlBuffer>,
              _x: i32,
              _y: i32)
    {
        self.s_attached_buffer = buf;
    }

    fn commit(&mut self)
    {
        // If there was no surface attached, do nothing
        if self.s_attached_buffer.is_none() {
            return; // throw error?
        }
        self.s_committed_buffer = self.s_attached_buffer.take();

        // Get the ShmBuffer from the user data so we
        // can read its contents
        let shm_buf = self.s_committed_buffer
            // this is a bit wonky, we need to get a reference
            // to committed, but it is behind an option
            .as_ref().unwrap()
            // now we can call as_ref on the &WlBuffer
            .as_ref()
            .user_data()
            .get::<ShmBuffer>()
            .unwrap();

        // ShmBuffer holds the base pointer and an offset, so
        // we need to get the actual pointer, which will be
        // wrapped in a MemImage
        let fb = shm_buf.get_mem_image();

        self.s_wm_tx.send(
            wm::task::Task::update_window_contents_from_mem(
                self.s_id, // ID of the new window
                fb,
                // window dimensions
                shm_buf.sb_width as usize,
                shm_buf.sb_height as usize,
            )
        ).unwrap();
    }

    pub fn destroy(&mut self) {
        self.s_wm_tx.send(
            wm::task::Task::close_window(self.s_id)
        ).unwrap();
    }

    // create a new visible surface at coordinates (x,y)
    // from the specified wayland resource
    pub fn new(id: u64,
               wm_tx: Sender<wm::task::Task>,
               x: u32,
               y: u32)
               -> Surface
    {
        Surface {
            s_id: id,
            s_attached_buffer: None,
            s_committed_buffer: None,
            s_x: x,
            s_y: y,
            s_wm_tx: wm_tx,
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.destroy();
    }
}
