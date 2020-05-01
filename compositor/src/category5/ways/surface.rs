// wl_surface interface
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
use ws::Main;
use ws::protocol::{wl_buffer, wl_surface as wlsi};

use super::super::vkcomp::wm;

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
    s_attached_buffer: Option<Main<wl_buffer::WlBuffer>>,
    // the s_attached_buffer is moved here to signify that we can draw
    // with it.
    pub s_committed_buffer: Option<Main<wl_buffer::WlBuffer>>,
    // the location of the surface in our compositor
    s_x: u32,
    s_y: u32,
    s_wm_tx: Sender<wm::task::Task>,
}

impl Surface {

    // // attach a wl_buffer to the surface
    // //
    // // The client crafts a buffer with care, and tells us that it will be
    // // backing the surface represented by `resource`. `buffer` will be
    // // placed in the private struct that the compositor made.
    // pub extern "C" fn attach(client: *mut wl_client,
    //                          resource: *mut wl_resource,
    //                          buffer: *mut wl_resource,
    //                          x: i32,
    //                          y: i32)
    // {
    //     // get our private struct and assign it the buffer
    //     // that the client has attached
    //     let mut surface = get_userdata_of_type!(resource, Surface).unwrap();
    //     surface.s_attached_buffer = Some(WLResource::from_ptr(buffer));
    // }

    // pub extern "C" fn commit(client: *mut wl_client,
    //                          resource: *mut wl_resource)
    // {
    //     // only do shm for now
    //     let mut surface = get_userdata_of_type!(resource, Surface).unwrap();
    //     // the wl_shm_buffer object, not the framebuffer
    //     if !surface.s_attached_buffer.is_none() {
    //         surface.s_committed_buffer = surface.s_attached_buffer;
    //     }

    //     let shm_buff = ws_shm_buffer_get(
    //         surface.s_committed_buffer.unwrap()
    //     );
    //     let fb = ws_shm_buffer_get_data(shm_buff);
    //     let width = fb.width;
    //     let height = fb.height;

    //     surface.s_wm_tx.send(
    //         wm::task::Task::update_window_contents_from_mem(
    //             surface.s_id, // ID of the new window
    //             fb,
    //             width, height, // window dimensions
    //         )
    //     ).unwrap();
    // }

    // pub extern "C" fn delete(resource: *mut wl_resource) {
    //     let surface = get_userdata_of_type!(resource, Surface).unwrap();

    //     surface.s_wm_tx.send(
    //         wm::task::Task::close_window(surface.s_id)
    //     ).unwrap();
    // }

    // create a new visible surface at coordinates (x,y)
    // from the specified wayland resource
    pub fn new(id: u64,
               _surf: Main<wlsi::WlSurface>,
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
