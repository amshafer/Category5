// Tasks are work orders passed from other subsystems
// to this one for processing. This describes those
// units of work.
//
// Austin Shafer - 2020
#![allow(dead_code)]

// Set the desktop background to the data
// held in `pixels`
pub struct SetBackgroundFromMem {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

// A unit of work to be handled by this subsystem
//
// This is usually an action that needs to
// be performed
pub enum Task {
    begin_frame,
    end_frame,
    set_background_from_mem(SetBackgroundFromMem),
}
