// Implementation of the wl_region interface for tracking
// arbitrary areas of the screen
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::protocol::wl_region;
use ws::Main;

use std::cell::RefCell;
use std::rc::Rc;
use utils::region::Rect;

/// The private userdata for the wl_region
#[derive(Debug)]
pub struct Region {
    /// A list of rectangles which make up the
    /// active portion of the region
    pub r_add: Vec<Rect<i32>>,
    /// List of rectangles to be subtracted from the
    /// active area
    pub r_sub: Vec<Rect<i32>>,
}

// Register a new wl_region
pub fn register_new(reg: Main<wl_region::WlRegion>) {
    let re = Rc::new(RefCell::new(Region {
        r_add: Vec::new(),
        r_sub: Vec::new(),
    }));
    reg.as_ref().user_data().set(|| re.clone());

    // register our request handler
    reg.quick_assign(move |_, r, _| {
        let mut nre = re.borrow_mut();
        nre.handle_request(r);
    });
}

impl Region {
    pub fn handle_request(&mut self, req: wl_region::Request) {
        match req {
            wl_region::Request::Add {
                x,
                y,
                width,
                height,
            } => self.r_add.push(Rect::new(x, y, width, height)),
            wl_region::Request::Subtract {
                x,
                y,
                width,
                height,
            } => self.r_sub.push(Rect::new(x, y, width, height)),
            // don't do anything special when destroying
            _ => (),
        }
    }

    /// Check if the point (x, y) is contained in this region
    pub fn intersects(&self, x: i32, y: i32) -> bool {
        // TODO: make this efficient
        let mut contains = false;
        for add in self.r_add.iter() {
            if add.intersects(x, y) {
                contains = true;
            }
        }

        if contains {
            // If any of the subtracted areas contain the
            // point, then we fail
            for sub in self.r_sub.iter() {
                if sub.intersects(x, y) {
                    contains = false;
                }
            }
        }

        return contains;
    }
}
