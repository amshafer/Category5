// The Category 5 wayland compositor
//
// Austin Shafer - 2020

mod vkcomp;
mod ways;

use ways::Compositor;
use vkcomp::wm;
use vkcomp::wm::*;

use std::sync::mpsc;
use std::sync::mpsc::{Sender};

// The category5 compositor
//
// This is the top layer of the storm
pub struct Category5 {
    // The wayland compositor
    // Kind of confusing since category5 is also
    // a compositor...
    //
    // Category5 - Graphical desktop compositor
    // ways::Compositor - wayland protocol compositor object 
    wc: Box<Compositor>,
    // send channel to give the wayland subsystem work
    // wc_tx: Sender<ways::Task>,
    // The window manager (vulkan rendering backend)
    wm: WindowManager,
    wm_tx: Sender<wm::task::Task>,
}

impl Category5 {
    // This is a cooler way of saying new
    pub fn spin() -> Category5 {
        let (wm_tx, wm_rx) = mpsc::channel();
        Category5 {
            // Get the wayland compositor
            // Note that the wayland compositor + vulkan renderer is the
            // complete compositor
            wc: Compositor::new(),
            // creates a context, swapchain, images, and others
            // initialize the pipeline, renderpasses, and display engine
            wm: WindowManager::new(wm_rx),
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
        let sb = wm::task::SetBackgroundFromMem {
            pixels: tex,
            width: tex_width,
            height: tex_height,
        };
        self.wm_tx.send(
            wm::task::Task::set_background_from_mem(sb)
        ).unwrap();
    }

    // This is the main loop of the entire system
    pub fn run_forever(&mut self) {
        loop {
            // wait for the next event
            self.wc.event_loop_dispatch();
            self.wc.flush_clients();

            // draw a frame to be displayed
            self.wm_tx.send(
                wm::task::Task::begin_frame
            ).unwrap();
            // present our frame to the screen
            self.wm_tx.send(
                wm::task::Task::begin_frame
            ).unwrap();
        }
    }
}
