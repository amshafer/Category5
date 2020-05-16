// Tasks are work orders passed from other subsystems
// to this one for processing. This describes those
// units of work.
//
// Austin Shafer - 2020
#![allow(dead_code)]

// This is the only place in vkcomp allowed to reference
// wayland. We will be sticking wayland objects in tasks
// to be released after the task is completed
extern crate wayland_server as ws;
use ws::protocol::wl_buffer;

use crate::category5::utils::{Dmabuf, MemImage};

// Tell wm the desktop background
//
// This basically just creates a mesh with the max
// depth that takes up the entire screen. We use
// the channel to dispatch work
pub struct SetBackgroundFromMem {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct MoveCursor {
    pub x: f64,
    pub y: f64,
}

// Window creation parameters
//
// Similar to how arguments are passed in vulkan, here
// we have a structure that holds all the arguments
// for creating a window.
pub struct CreateWindow {
    // ID of the window
    pub id: u64,
    // Window position
    pub x: u32,
    pub y: u32,
    // The size of the window (in pixels)
    pub window_width: u32,
    pub window_height: u32,
}

pub struct UpdateWindowContentsFromDmabuf {
    pub ufd_id: u64,
    // dmabuf from linux_dmabuf protocol
    pub ufd_dmabuf: Dmabuf,
    // private: the wl_buffer to release when this
    // is handled. pixels belongs to this.
    ufd_wl_buffer: wl_buffer::WlBuffer,
}

impl Drop for UpdateWindowContentsFromDmabuf {
    fn drop(&mut self) {
        self.ufd_wl_buffer.release();
    }
}

pub struct UpdateWindowContentsFromMem {
    pub id: u64,
    // The resolution of the texture
    pub width: usize,
    pub height: usize,
    // Memory region to copy window contents from
    pub pixels: MemImage,
    // private: the wl_buffer to release when this
    // is handled. pixels belongs to this.
    ufm_wl_buffer: wl_buffer::WlBuffer,
}

impl Drop for UpdateWindowContentsFromMem {
    fn drop(&mut self) {
        self.ufm_wl_buffer.release();
    }
}

// A unit of work to be handled by this subsystem
//
// This is usually an action that needs to
// be performed
pub enum Task {
    begin_frame,
    end_frame,
    close_window(u64),
    mc(MoveCursor),
    sbfm(SetBackgroundFromMem),
    cw(CreateWindow),
    uwcfd(UpdateWindowContentsFromDmabuf),
    uwcfm(UpdateWindowContentsFromMem),
}

impl Task {
    pub fn set_background_from_mem(tex: Vec<u8>,
                                   tex_width: u32,
                                   tex_height: u32)
                                   -> Task
    {
        Task::sbfm(SetBackgroundFromMem {
            pixels: tex,
            width: tex_width,
            height: tex_height,
        })
    }

    pub fn move_cursor(x: f64, y: f64) -> Task {
        Task::mc(MoveCursor {
            x: x,
            y: y,
        })
    }

    pub fn create_window(id: u64,
                         x: u32,
                         y: u32,
                         window_width: u32,
                         window_height: u32)
                         -> Task
    {
        Task::cw(CreateWindow {
            id: id,
            x: x,
            y: y,
            window_width: window_width,
            window_height: window_height,
        })
    }

    pub fn update_window_contents_from_dmabuf(id: u64,
                                              dmabuf: Dmabuf,
                                              buffer: wl_buffer::WlBuffer)
                                              -> Task
    {
        Task::uwcfd(UpdateWindowContentsFromDmabuf {
            ufd_id: id,
            ufd_dmabuf: dmabuf,
            ufd_wl_buffer: buffer,
        })
    }

    pub fn update_window_contents_from_mem(id: u64,
                                           tex: MemImage,
                                           buffer: wl_buffer::WlBuffer,
                                           tex_width: usize,
                                           tex_height: usize)
                                           -> Task
    {
        Task::uwcfm(UpdateWindowContentsFromMem {
            id: id,
            width: tex_width,
            height: tex_height,
            pixels: tex,
            ufm_wl_buffer: buffer,
        })
    }
}
