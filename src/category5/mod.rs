// The Category 5 wayland compositor
//
// Austin Shafer - 2020
extern crate thundr;
extern crate utils as cat5_utils;
extern crate wayland_server as ws;

mod atmosphere;
mod input;
mod vkcomp;
mod ways;

use crate::category5::input::Input;
use atmosphere::Atmosphere;
use cat5_utils::{fdwatch::FdWatch, log, timing::*};
use thundr::ThundrError;
use vkcomp::wm::*;

use ws::protocol::{
    wl_compositor as wlci, wl_data_device_manager as wlddm, wl_output, wl_seat, wl_shell, wl_shm,
    wl_subcompositor as wlsc, wl_surface as wlsi,
};

use std::thread;
use std::time::Duration;

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

/// This is our big state dispatch struct for wayland_server
///
/// Wayland_server works by indexing off of one large State struct
/// to "delegate" structs that implement the Dispatch trait. This
/// state struct holds all of the smaller structs that actually
/// handle protocol events, and we will forward requests to said
/// substructs with the delegate_dispatch! macro family.
pub struct Climate {
    /// The big database of all our properties
    c_atmos: Atmosphere,
    /// The input subsystem
    c_input: Input,
}

impl Climate {
    fn new() -> Self {
        Self {
            c_atmos: Atmosphere::new(),
            c_input: Input::new(),
        }
    }
}

/// The event manager
///
/// This class the launching point of the wayland stack. It
/// is used by category5 to dispatch handling and listen
/// on the wayland fds. It also owns the wayland-rs top
/// level object in em_display
pub struct EventManager {
    /// Our wayland-rs state dispatcher
    em_climate: Climate,
    em_wm: WindowManager,
    /// The wayland display object, this is the core
    /// global singleton for libwayland
    em_display: ws::Display<Climate>,
    em_dh: ws::DisplayHandle,
    /// How much the mouse has moved in this frame
    /// aggregates input pointer events
    em_pointer_dx: f64,
    em_pointer_dy: f64,
}

impl EventManager {
    /// Returns a new struct in charge of running the main event loop
    ///
    /// This creates a new wayland compositor, setting up all
    /// the needed resources for the struct. It will create a
    /// wl_display, initialize a new socket, create the client/surface
    ///  lists, and create a compositor global resource.
    ///
    /// This kicks off the global callback chain, starting with
    ///    Compositor::bind_compositor_callback
    pub fn new() -> Box<EventManager> {
        let mut display = ws::Display::new();
        display
            .add_socket_auto()
            .expect("Failed to add a socket to the wayland server");
        let display_handle = display.handle();

        // Our big state holder for wayland-rs
        let state = Climate::new();

        let mut evman = Box::new(EventManager {
            em_wm: WindowManager::new(&mut state.c_atmos),
            em_climate: state,
            em_display: display,
            em_dh: display_handle.clone(),
            em_pointer_dx: 0.0,
            em_pointer_dy: 0.0,
        });

        // Register our global interfaces that will be advertised to all clients
        // --------------------------
        // wl_compositor
        display_handle.create_global::<Climate, wlci::WlCompositor, ()>(4, ());

        return evman;
    }

    /// Each subsystem has a function that implements its main
    /// loop. This is that function
    pub fn worker_thread(&mut self) {
        // We want to track every 15ms. This is a little less than
        // once per 60fps frame. It doesn't have to be exact, but
        // we need to send certain updates to vkcomp roughly once per
        // frame
        let mut tm = TimingManager::new(15);

        // wayland-rs will not do blocking for us,
        // When registered, these will tell kqueue to notify
        // use when the wayland or libinput fds are readable
        let mut fdw = FdWatch::new();
        fdw.add_fd(self.em_display.get_poll_fd());
        fdw.add_fd(self.em_climate.c_input.get_poll_fd());
        // now register the fds we added
        fdw.register_events();

        // reset the timer before we start
        tm.reset();
        let mut needs_render = true;
        while needs_render || fdw.wait_for_events() {
            log::profiling!("starting loop");
            // First thing to do is to dispatch libinput
            // It has time sensitive operations which need to take
            // place as soon as the fd is readable
            self.em_climate
                .c_input
                .dispatch(&mut self.em_climate.c_atmos);

            // TODO: fix frame timings to prevent the current state of
            // 3 frames of latency
            //
            // The input subsystem has batched the changes to the window
            // due to resizing, we need to send those changes now
            self.em_climate.c_input.update_from_eventloop();

            {
                // Try to flip hemispheres to push our updates to vkcomp
                // If we can't recieve it, vkcomp isn't ready, and we should
                // continue processing wayland updates so the system
                // doesn't lag
                let mut atmos = &mut self.em_climate.c_atmos;
                if atmos.is_changed() {
                    atmos.clear_changed();
                    needs_render = true;
                }

                if needs_render {
                    log::profiling!("trying to render frame");
                    match self.em_wm.render_frame(&mut *atmos) {
                        Ok(()) => needs_render = false,
                        Err(e) => {
                            if let Some(err) = e.downcast_ref::<ThundrError>() {
                                if *err == ThundrError::NOT_READY || *err == ThundrError::TIMEOUT {
                                    // ignore the timeout, start our loop over
                                    log::profiling!("Next frame isn't ready, continuing");
                                } else {
                                    panic!("Rendering a frame failed with {:?}", e);
                                }
                            }
                        }
                    };
                }
            }

            // wait for the next event
            self.em_display.dispatch_clients(&self.em_climate).unwrap();
            self.em_display.flush_clients();

            log::profiling!("EventManager: Blocking for max {} ms", tm.time_remaining());
        }
    }
}
