// Implementation of the wl_output interface
//
// wl_output advertises what (physical) displays are available
// for clients to present surfaces on
//
// Austin Shafer 2020
extern crate wayland_server as ws;

use crate::category5::atmosphere::Atmosphere;
use ws::protocol::wl_output;
use ws::protocol::wl_output::{Mode, Subpixel, Transform};
use ws::Main;

use std::sync::{Arc, Mutex};

// TODO: have vkcomp give us more information to relay
pub fn wl_output_broadcast(cell: Arc<Mutex<Atmosphere>>, out: Main<wl_output::WlOutput>) {
    let atmos = cell.lock().unwrap();
    let res = atmos.get_resolution();

    // send geometry
    out.geometry(
        0,
        0,
        res.0 as i32,
        res.1 as i32,
        Subpixel::Unknown,
        "monitor".to_string(),
        "".to_string(),
        Transform::Normal,
    );

    out.mode(
        Mode::Current,
        res.0 as i32,
        res.1 as i32,
        60, // 60 Hz default
    );

    // let the client know we are done with the monitor config
    out.done();
}
