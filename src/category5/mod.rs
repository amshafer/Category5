// The Category 5 wayland compositor
//
// Austin Shafer - 2020

mod atmosphere;
mod input;
mod vkcomp;
mod ways;

use ways::compositor::EventManager;

use std::thread;

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
}

impl Category5 {
    // This is a cooler way of saying new
    // I got bored of writing new constantly
    pub fn spin() -> Category5 {
        Category5 {
            // Get the wayland compositor
            // Note that the wayland compositor + vulkan renderer
            // is the complete compositor
            c5_wc: Some(
                thread::Builder::new()
                    .name("wayland_compositor".to_string())
                    .spawn(|| {
                        let mut ev = EventManager::new();
                        ev.worker_thread();
                    })
                    .unwrap(),
            ),
        }
    }

    // This is the main loop of the entire system
    // We just wait for the other threads
    pub fn run_forever(&mut self) {
        self.c5_wc.take().unwrap().join().ok();
    }
}
