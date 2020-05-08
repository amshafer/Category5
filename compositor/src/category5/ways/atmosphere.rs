// Global atmosphere
//
// Austin Shafer - 2020
use super::surface::*;

use std::vec::Vec;
use std::rc::Rc;
use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

// Global state tracking
//
// Don't make fun of my naming convention pls. We need a
// place for all wayland code to stash meta information.
// This is such a place, but it should not hold anything
// exceptionally protocol-specific for sync reasons.
//
// This is referenced by all protocol handlers
#[allow(dead_code)]
pub struct Atmosphere {
    // placeholder for now
    pub a_desktop: u32,
    // A list of surfaces which have been handed out to clients
    // Recorded here so we can perform interesting DE interactions
    pub a_surfaces: Vec<Rc<RefCell<Surface>>>,
}

impl Atmosphere {
    pub fn new() -> Atmosphere {
        Atmosphere {
            a_desktop: 0,
            a_surfaces: Vec::new(),
        }
    }

    pub fn signal_frame_callbacks(&mut self) {
        for cell in &self.a_surfaces {
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
