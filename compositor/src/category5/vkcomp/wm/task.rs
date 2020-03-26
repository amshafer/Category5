// Tasks are work orders passed from other subsystems
// to this one for processing. This describes those
// units of work.
//
// Austin Shafer - 2020
#![allow(dead_code)]

use crate::category5::utils::MemImage;

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

pub struct UpdateWindowContentsFromMem {
    pub id: u64,
    // Memory region to copy window contents from
    pub pixels: MemImage,
    // The resolution of the texture
    pub width: usize,
    pub height: usize,
}


// A unit of work to be handled by this subsystem
//
// This is usually an action that needs to
// be performed
pub enum Task {
    begin_frame,
    end_frame,
    close_window(u64),
    sbfm(SetBackgroundFromMem),
    cw(CreateWindow),
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

    pub fn update_window_contents_from_mem(id: u64,
                                           tex: MemImage,
                                           tex_width: usize,
                                           tex_height: usize)
                                           -> Task
    {
        Task::uwcfm(UpdateWindowContentsFromMem {
            id: id,
            pixels: tex,
            width: tex_width,
            height: tex_height,
        })
    }
}
