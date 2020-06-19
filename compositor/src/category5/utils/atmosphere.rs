// Global atmosphere
//
// Austin Shafer - 2020
use crate::category5::ways::surface::*;
use crate::category5::vkcomp::wm;

use std::rc::Rc;
use std::vec::Vec;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::mpsc::{Sender,Receiver};
use std::time::{SystemTime, UNIX_EPOCH};

// Represents updating one property in the ECS
//
// Our atmosphere is really just a lock-free Entity
// component set. We need a way to snapshot the
// changes accummulated in a hemisphere during a frame
// so that we can replay them on the other hemisphere
// to keep things consistent. This encapsulates uppdating
// one property.
//
// These will be collected in a hashmap for replay
//    map<(window id, property id), Patch>
#[allow(dead_code)]
enum Patch {
    cursor_position(f64, f64),
}

// Global state tracking
//
// Don't make fun of my naming convention pls. We need a
// place for all wayland code to stash meta information.
// This is such a place, but it should not hold anything
// exceptionally protocol-specific for sync reasons.
//
// Although this is referenced everywhere, both sides
// will have their own version of it. a_hemisphere is
// the shared part.
//
// Keep in mind this only holds any shared data, data
// exclusive to subsystems will be held by said subsystem
#[allow(dead_code)]
pub struct Atmosphere {
    // transfer channel
    a_tx: Sender<Box<Hemisphere>>,
    // receive channel
    a_rx: Receiver<Box<Hemisphere>>,
    // The current hemisphere
    //
    // While this is held we will retain ownership of the
    // current hemisphere's mutex, and be able to service
    // requests on this hemisphere
    //
    // To switch hemispheres we send this through the
    // channel to the other subsystem.
    a_hemi: Option<Box<Hemisphere>>,

    // -- private subsystem specific resources --

    // -- ways --
    // a list of surfaces to have their callbacks called
    a_ways_surfaces: Vec<Rc<RefCell<Surface>>>,
}

impl Atmosphere {
    // Create a new atmosphere to be shared within a subsystem
    //
    // We pass in the hemispheres since they will have to
    // also be passed to the other subsystem.
    // One subsystem must be setup as index 0 and the other
    // as index 1
    pub fn new(tx: Sender<Box<Hemisphere>>,
               rx: Receiver<Box<Hemisphere>>)
               -> Atmosphere
    {
        Atmosphere {
            a_tx: tx,
            a_rx: rx,
            a_hemi: Some(Box::new(Hemisphere::new())),
            // TODO: only do this for ways
            a_ways_surfaces: Vec::new(),
        }
    }

    pub fn flip_hemispheres(&mut self) {
        // first grab our own hemi
        if let Some(h) = self.a_hemi.take() {
            self.a_tx.send(h)
                .expect("Could not send hemisphere");
            let new_hemi = self.a_rx.recv()
                .expect("Could not recv hemisphere");

            // Replace with the hemisphere from the
            // other subsystem
            self.a_hemi = Some(new_hemi);
        }
    }

    // ------------------------------
    // For the sake of abstraction, the atmosphere will be the
    // point of contact for modifying global state. We will
    // record any changes to replay and pass the data down
    // to the hemisphere
    // ------------------------------

    pub fn add_window_id(&mut self, id: u32) {
        self.a_hemi.as_mut().map(|h| h.add_window_id(id));
    }

    pub fn add_wm_task(&mut self, task: wm::task::Task) {
        self.a_hemi.as_mut().map(|h| h.add_wm_task(task));
    }

    pub fn get_next_wm_task(&mut self) -> Option<wm::task::Task> {
        self.a_hemi.as_mut().unwrap().wm_task_pop()
    }

    // -- subsystem specific handlers --

    pub fn add_surface(&mut self, surf: Rc<RefCell<Surface>>) {
        self.a_ways_surfaces.push(surf);
    }

    pub fn signal_frame_callbacks(&mut self) {
        for cell in &self.a_ways_surfaces {
            let surf = cell.borrow_mut();
            if let Some(callback) = surf.s_frame_callback.as_ref() {
                // frame callbacks return the current time
                // in milliseconds.
                callback.done(SystemTime::now()
                              .duration_since(UNIX_EPOCH)
                              .expect("Error getting system time")
                              .as_millis() as u32);
            }
        }
    }
}

// One hemisphere of the bicameral atmosphere
//
// The atmosphere is the global state, but it needs to be
// simultaneously accessed by two threads. We have two
// hemispheres, each of which is a entity component set
// that holds the current state of the desktop(s).
//
// It's like rcu done through double buffering. At the
// end of each frame both threads synchronize and switch
// hemispheres.
//
// Each subsystem (ways and vkcomp) will possess one
// hemisphere. ways will update its hemisphere and
// vkcomp will construct a frame from its hemisphere
//
// Following Abrash's advice of "know your data" I am
// using a vector instead of a hashmap for the main table.
// The "keys" (aka window ids) are offsets into the vec.
// This is done since there are normally < 15 windows
// open on any given desktop, and this is the largest
// table so we are going for compactness. The offsets
// still provide O(1) lookup time, with the downside
// that we have to scan the vec to find a new entry,
// and potentially resize the vec to fit a new one.
#[allow(dead_code)]
pub struct Hemisphere {
    // A list of surfaces which have been handed out to clients
    // Recorded here so we can perform interesting DE interactions
    h_windows: Vec<u32>,
    // a list of the window ids from front to back
    // index 0 is the current focus
    h_window_heir: Vec<u32>,
    // A list of tasks to be completed by vkcomp this frame
    //
    // Tasks are one time events. Anything related to state should
    // be added elsewhere. A task is a transfer of ownership from
    // ways to vkcommp
    h_wm_tasks: Vec<wm::task::Task>,
}


impl Hemisphere {
    fn new() -> Hemisphere {
        Hemisphere {
            h_windows: Vec::new(),
            h_window_heir: Vec::new(),
            h_wm_tasks: Vec::new(),
        }
    }

    fn add_wm_task(&mut self, task: wm::task::Task) {
        self.h_wm_tasks.push(task);
    }

    fn add_window_id(&mut self, id: u32) {
        self.h_windows.push(id);
    }

    pub fn wm_task_pop(&mut self) -> Option<wm::task::Task> {
        self.h_wm_tasks.pop()
    }
}
