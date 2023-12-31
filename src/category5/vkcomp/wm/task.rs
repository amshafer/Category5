// Tasks are work orders passed from other subsystems
// to this one for processing. This describes those
// units of work.
//
// Austin Shafer - 2020
#![allow(dead_code)]
use std::fmt;
use std::sync::Arc;

// This is the only place in vkcomp allowed to reference
// wayland. We will be sticking wayland objects in tasks
// to be released after the task is completed
extern crate wayland_server as ws;
use ws::protocol::wl_buffer;

extern crate utils;
use utils::log;
use utils::{Dmabuf, MemImage};

use crate::category5::atmosphere::SurfaceId;

// Tell wm the desktop background
//
// This basically just creates a mesh with the max
// depth that takes up the entire screen. We use
// the channel to dispatch work
#[derive(Debug)]
pub struct SetBackgroundFromMem {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct UpdateWindowContentsFromDmabuf {
    pub ufd_id: SurfaceId,
    // dmabuf from linux_dmabuf protocol
    pub ufd_dmabuf: Arc<Dmabuf>,
    // private: the wl_buffer to release when this
    // is handled. pixels belongs to this.
    pub ufd_wl_buffer: wl_buffer::WlBuffer,
}

impl fmt::Debug for UpdateWindowContentsFromDmabuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateWindowContentsFromDmabuf")
            .field("ufd_id", &format!("{:?}", self.ufd_id))
            .field("ufd_dmabuf", &format!("{:?}", self.ufd_dmabuf))
            .field("ufd_wl_buffer", &"<wl_buffer omitted>".to_string())
            .finish()
    }
}

pub struct UpdateWindowContentsFromMem {
    pub id: SurfaceId,
    // The resolution of the texture
    pub width: usize,
    pub height: usize,
    // Memory region to copy window contents from
    pub pixels: MemImage,
    // private: the wl_buffer to release when this
    // is handled. pixels belongs to this.
    ufm_wl_buffer: wl_buffer::WlBuffer,
}

impl fmt::Debug for UpdateWindowContentsFromMem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpdateWindowContentsFromMem")
            .field("id", &format!("{:?}", self.id))
            .field("width", &format!("{:?}", self.width))
            .field("height", &format!("{:?}", self.height))
            .field("pixels", &"<MemImage omitted>".to_string())
            .field("ufd_wl_buffer", &"<wl_buffer omitted>".to_string())
            .finish()
    }
}

impl Drop for UpdateWindowContentsFromMem {
    fn drop(&mut self) {
        log::profiling!("Releasing shm buffer");
        self.ufm_wl_buffer.release();
    }
}

// A unit of work to be handled by this subsystem
//
// This is usually an action that needs to
// be performed
#[derive(Debug)]
pub enum Task {
    close_window(SurfaceId),
    move_to_front(SurfaceId),
    new_toplevel(SurfaceId),
    new_subsurface { id: SurfaceId, parent: SurfaceId },
    place_subsurface_above { id: SurfaceId, other: SurfaceId },
    place_subsurface_below { id: SurfaceId, other: SurfaceId },
    uwcfd(UpdateWindowContentsFromDmabuf),
    uwcfm(UpdateWindowContentsFromMem),
    set_cursor { id: Option<SurfaceId> },
    reset_cursor,
}

impl Task {
    pub fn update_window_contents_from_dmabuf(
        id: SurfaceId,
        dmabuf: Arc<Dmabuf>,
        buffer: wl_buffer::WlBuffer,
    ) -> Task {
        Task::uwcfd(UpdateWindowContentsFromDmabuf {
            ufd_id: id,
            ufd_dmabuf: dmabuf,
            ufd_wl_buffer: buffer,
        })
    }

    pub fn update_window_contents_from_mem(
        id: SurfaceId,
        tex: MemImage,
        buffer: wl_buffer::WlBuffer,
        tex_width: usize,
        tex_height: usize,
    ) -> Task {
        Task::uwcfm(UpdateWindowContentsFromMem {
            id: id,
            width: tex_width,
            height: tex_height,
            pixels: tex,
            ufm_wl_buffer: buffer,
        })
    }
}
