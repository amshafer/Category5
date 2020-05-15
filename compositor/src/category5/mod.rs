// The Category 5 wayland compositor
//
// Austin Shafer - 2020

mod vkcomp;
mod ways;
mod utils;
mod input;

use vkcomp::wm;
use vkcomp::wm::*;
use ways::compositor::Compositor;
use self::input::Input;

use std::thread;
use std::sync::mpsc;
use std::sync::mpsc::{Sender};

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
    c5_wc_tx: Sender<ways::task::Task>,
    // The graphics subsystem
    //
    // send channel to give the wayland subsystem work
    // wc_tx: Sender<ways::Task>,
    // The window manager (vulkan rendering backend)
    c5_wm: Option<thread::JoinHandle<()>>,
    c5_wm_tx: Sender<wm::task::Task>,
    // The input sybsystem
    //
    // It does not have a channel because it only produces
    // events for the other subsystems.
    c5_input: Option<thread::JoinHandle<()>>,
}

impl Category5 {
    // This is a cooler way of saying new
    // I got bored of writing new constantly
    pub fn spin() -> Category5 {
        // The original channels
        let (wc_tx, wc_rx) = mpsc::channel();
        let (wm_tx, wm_rx) = mpsc::channel();
        // A clone used for ways
        let wm_tx_clone = wm_tx.clone();
        // Clones used for input
        let wm_tx_clone2 = wm_tx.clone();
        let wc_tx_clone = wc_tx.clone();

        Category5 {
            // Get the wayland compositor
            // Note that the wayland compositor + vulkan renderer
            // is the complete compositor
            c5_wc: Some(thread::Builder::new()
                        .name("wayland_handlers".to_string())
                        .spawn(|| {
                let mut ev = Compositor::new(wc_rx, wm_tx_clone);
                ev.worker_thread();
            }).unwrap()),
            c5_wc_tx: wc_tx,
            // creates a context, swapchain, images, and others
            // initialize the pipeline, renderpasses, and
            // display engine.
            c5_wm: Some(thread::Builder::new()
                        .name("vulkan_compositor".to_string())
                        .spawn(|| {
                let mut wm = WindowManager::new(wm_rx);
                wm.worker_thread();
            }).unwrap()),
            c5_wm_tx: wm_tx,
            c5_input: Some(thread::Builder::new()
                        .name("input".to_string())
                        .spawn(|| {
                let mut input = Input::new(wc_tx_clone, wm_tx_clone2);
                input.worker_thread();
            }).unwrap()),
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
        self.c5_wm_tx.send(
            wm::task::Task::set_background_from_mem(
                tex, tex_width, tex_height
            )
        ).unwrap();
    }

    // This is the main loop of the entire system
    // We just wait for the other threads
    pub fn run_forever(&mut self) {
        self.c5_wc.take().unwrap().join().ok();
        self.c5_wm.take().unwrap().join().ok();
        self.c5_input.take().unwrap().join().ok();
    }
}
