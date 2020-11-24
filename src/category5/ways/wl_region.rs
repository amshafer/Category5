// Implementation of the wl_region interface for tracking
// arbitrary areas of the screen
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::wl_region;

use utils::region::Rect;
use std::rc::Rc;
use std::cell::RefCell;

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
    pub fn handle_request(&mut self,
                          req: wl_region::Request)
    {
        match req {
            wl_region::Request::Add { x, y, width, height } =>
                self.r_add.push(Rect::new(x, y, width, height)),
            wl_region::Request::Subtract { x, y, width, height } =>
                self.r_sub.push(Rect::new(x, y, width, height)),
            // don't do anything special when destroying
            _ => (),
        }
    }
}
