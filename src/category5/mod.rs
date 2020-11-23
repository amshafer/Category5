// The Category 5 wayland compositor
//
// Austin Shafer - 2020

mod vkcomp;
mod ways;
mod input;
mod atmosphere;

use vkcomp::wm::*;
use ways::compositor::EventManager;

use std::thread;
use std::sync::mpsc;

// The category5 compositor
//
// This is the top layer of the storm
// Instead of holding subsystem structures, it holds
// thread handles that the subsystems are running in.
#[allow(dead_code)]
pub struct Category5 {
    // The wayland subsystem
    //
    // Category5 - Graphical desktop compositor
    // ways::Compositor - wayland protocol compositor object 
    c5_wc: Option<thread::JoinHandle<()>>,
    // The graphics subsystem
    //
    // The window manager (vulkan rendering backend)
    c5_wm: Option<thread::JoinHandle<()>>,
}

impl Category5 {
    // This is a cooler way of saying new
    // I got bored of writing new constantly
    pub fn spin() -> Category5 {
        // vkcomp to ways channel
        let (v2w_tx, v2w_rx) = mpsc::channel();
        // ways to vkcomp channel
        let (w2v_tx, w2v_rx) = mpsc::channel();

        Category5 {
            // Get the wayland compositor
            // Note that the wayland compositor + vulkan renderer
            // is the complete compositor
            c5_wc: Some(thread::Builder::new()
                        .name("wayland_handlers".to_string())
                        .spawn(|| {
                let mut ev = EventManager::new(w2v_tx, v2w_rx);
                ev.worker_thread();
            }).unwrap()),
            // creates a context, swapchain, images, and others
            // initialize the pipeline, renderpasses, and
            // display engine.
            c5_wm: Some(thread::Builder::new()
                        .name("vulkan_compositor".to_string())
                        .spawn(|| {
                let mut wm = WindowManager::new(v2w_tx, w2v_rx);
                wm.worker_thread();
            }).unwrap()),
        }
    }

    // This is the main loop of the entire system
    // We just wait for the other threads
    pub fn run_forever(&mut self) {
        self.c5_wc.take().unwrap().join().ok();
        self.c5_wm.take().unwrap().join().ok();
    }
}
