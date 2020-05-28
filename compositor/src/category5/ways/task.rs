// Tasks are work orders passed from other subsystems
// to this one for processing. This describes those
// units of work.
//
// Austin Shafer - 2020
#![allow(dead_code)]

// Grab an application
//
// This is the downpress on the mouse. It brings
// focus to the target application.
// If a MoveCursor occurs while grabbed, then the
// application will also be moved.
pub struct Grab {
    // id of the App to grab
    pub g_id: u64,
}

// Stop Grabbing an application
//
// This is the uppress on the mouse.
pub struct UnGrab {
    // id of the App to stop grabbing
    pub ug_id: u64,
}

// A unit of work to be handled by this subsystem
//
// This is usually an action that needs to
// be performed
pub enum Task {
    gr(Grab),
    ungr(UnGrab),
}

impl Task {
    pub fn grab(id: u64) -> Task {
        Task::gr(Grab {
            g_id: id,
        })
    }

    pub fn ungrab(id: u64) -> Task {
        Task::ungr(UnGrab {
            ug_id: id,
        })
    }
}
