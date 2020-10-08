// Implementation of the wl_region interface for tracking
// arbitrary areas of the screen
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::wl_region;

use crate::category5::utils::region::Rect;
use std::rc::Rc;
use std::cell::RefCell;

/// The private userdata for the wl_region
pub struct Region {
    /// A list of rectangles which make up the
    /// active portion of the region
    pub r_add: Vec<Rect>,
    /// List of rectangles to be subtracted from the
    /// active area
    pub r_sub: Vec<Rect>,
}

// Register a new wl_region
pub fn register_new(reg: Main<wl_region::WlRegion>) {
    let r = Rc::new(RefCell::new(Region {
        r_add: Vec::new(),
        r_sub: Vec::new(),
    }));
    reg.as_ref().user_data().set(move || r);
}

impl Region {
    pub fn handle_request(&mut self,
                          req: wl_region::Request,
                          reg: Main<wl_region::WlRegion>)
    {
        match req {
            wl_region::Request::Add { x, y, width, height } => {
                self.r_add.push(Rect {
                    r_start: Offset2D {
                        x: x, y: y
                    },
                    r_start: Offset2D {
                        x: width, y: height,
                    },
                });
            },
            wl_region::Request::Subtract { x, y, width, height } => {
                self.r_sub.push(Rect {
                    r_start: Offset2D {
                        x: x, y: y
                    },
                    r_start: Offset2D {
                        x: width, y: height,
                    },
                });
            },
            // don't do anything special when destroying
            _ => (),
        }
    }
}
