// The Category 5 wayland compositor
//
// Austin Shafer - 2020

mod vkcomp;
mod ways;
mod utils;

use ways::Compositor;
use vkcomp::wm;
use vkcomp::wm::*;

use std::thread;
use std::sync::mpsc;
use std::sync::mpsc::{Sender};

// The category5 compositor
//
// This is the top layer of the storm
#[allow(dead_code)]
pub struct Category5 {
    // The wayland compositor
    // Kind of confusing since category5 is also
    // a compositor...
    //
    // Category5 - Graphical desktop compositor
    // ways::Compositor - wayland protocol compositor object 
    wc: thread::JoinHandle<()>,
    wc_tx: Sender<ways::task::Task>,
    // send channel to give the wayland subsystem work
    // wc_tx: Sender<ways::Task>,
    // The window manager (vulkan rendering backend)
    wm: thread::JoinHandle<()>,
    wm_tx: Sender<wm::task::Task>,
}

impl Category5 {
    // This is a cooler way of saying new
    pub fn spin() -> Category5 {
        let (wc_tx, wc_rx) = mpsc::channel();
        let (wm_tx, wm_rx) = mpsc::channel();
        let wm_tx_clone = wm_tx.clone();
        Category5 {
            // Get the wayland compositor
            // Note that the wayland compositor + vulkan renderer is the
            // complete compositor
            wc: thread::spawn(|| {
                let mut wc = Compositor::new(wc_rx, wm_tx_clone);
                wc.worker_thread();
            }),
            wc_tx: wc_tx,
            // creates a context, swapchain, images, and others
            // initialize the pipeline, renderpasses, and display engine
            wm: thread::spawn(|| {
                let mut wm = WindowManager::new(wm_rx);
                wm.worker_thread();
            }),
            wm_tx: wm_tx,
        }
    }

    // Tell wm the desktop background
    //
    // This basically just creates a mesh with the max
    // depth that takes up the entire screen. We use
    // the channel to dispatch work
    pub fn set_background_from_mem(&self,
                                   tex: Vec<u8>,
                                   tex_width: u32,
                                   tex_height: u32)
    {
        self.wm_tx.send(
            wm::task::Task::set_background_from_mem(
                tex, tex_width, tex_height
            )
        ).unwrap();
    }

    // This is the main loop of the entire system
    pub fn run_forever(&mut self) {
        loop {
            // draw a frame to be displayed
            self.wm_tx.send(
                wm::task::Task::begin_frame
            ).unwrap();
            // present our frame to the screen
            self.wm_tx.send(
                wm::task::Task::end_frame
            ).unwrap();
        }
    }
}
