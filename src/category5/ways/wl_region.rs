// Implementation of the wl_region interface for tracking
// arbitrary areas of the screen
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use crate::category5::Climate;
use utils::region::Rect;
use ws::protocol::wl_region;

use std::sync::{Arc, Mutex};

// Register a new wl_region
pub fn register_new(id: ws::New<wl_region::WlRegion>, data_init: &mut ws::DataInit<'_, Climate>) {
    let re = Arc::new(Mutex::new(Region {
        r_add: Vec::new(),
        r_sub: Vec::new(),
    }));
    data_init.init(id, re);
}

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wl_region::WlRegion, Arc<Mutex<Region>>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_region::WlRegion,
        request: wl_region::Request,
        data: &Arc<Mutex<Region>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data.lock().unwrap().handle_request(request);
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &Arc<Mutex<Region>>,
    ) {
    }
}

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
