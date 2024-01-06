// Tasks are work orders passed from other subsystems
// to this one for processing. This describes those
// units of work.
//
// Austin Shafer - 2020
#![allow(dead_code)]
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
    set_cursor { id: Option<SurfaceId> },
    reset_cursor,
}
