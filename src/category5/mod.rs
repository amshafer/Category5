// The Category 5 wayland compositor
//
// Austin Shafer - 2020
extern crate thundr;
extern crate utils as cat5_utils;
extern crate wayland_protocols;
extern crate wayland_server as ws;

mod atmosphere;
mod input;
mod vkcomp;
mod ways;

use crate::category5::input::HWInput;
use atmosphere::Atmosphere;
use cat5_utils::{fdwatch::FdWatch, log, timing::*};
use thundr::ThundrError;
use utils::ClientId;
use vkcomp::wm::*;

use wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_v1 as zldv1;
use wayland_protocols::xdg::shell::server::*;
use ways::protocol::wl_drm::wl_drm;
use ws::protocol::{wl_compositor as wlci, wl_output, wl_seat, wl_shell, wl_shm, wl_subcompositor};

use std::ops::DerefMut;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};
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

/// This is our big state dispatch struct for wayland_server
///
/// Wayland_server works by indexing off of one large State struct
/// to "delegate" structs that implement the Dispatch trait. This
/// state struct holds all of the smaller structs that actually
/// handle protocol events, and we will forward requests to said
/// substructs with the delegate_dispatch! macro family.
pub struct Climate {
    /// The big database of all our properties
    c_atmos: Arc<Mutex<Atmosphere>>,
    /// The input subsystem
    c_input: HWInput,
}

impl Climate {
    fn new() -> Self {
        Self {
            c_atmos: Arc::new(Mutex::new(Atmosphere::new())),
            c_input: HWInput::new(),
        }
    }
}

/// Wayland client private data
///
/// This holds the client's id, along with a copy of Atmosphere
/// to clean up after itself
pub struct ClientInfo {
    ci_id: ClientId,
    ci_atmos: Arc<Mutex<Atmosphere>>,
}

impl ws::backend::ClientData for ClientInfo {
    fn initialized(&self, _client_id: ws::backend::ClientId) {}
    fn disconnected(
        &self,
        _client_id: ws::backend::ClientId,
        _reason: ws::backend::DisconnectReason,
    ) {
        // when the client is destroyed we need to tell the atmosphere
        // to free the reserved space
        self.ci_atmos.lock().unwrap().free_client_id(self.ci_id);
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
    /// The wayland unix socket
    em_socket: ws::ListeningSocket,
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
        let display = ws::Display::new().expect("Could not create wayland display");
        let display_handle = display.handle();

        // Our big state holder for wayland-rs
        let state = Climate::new();
        let wm = WindowManager::new(state.c_atmos.lock().unwrap().deref_mut());

        let evman = Box::new(EventManager {
            em_wm: wm,
            em_climate: state,
            em_display: display,
            em_socket: ws::ListeningSocket::bind_auto("wayland", 0..9)
                .expect("Could not create wayland socket"),
        });

        // Register our global interfaces that will be advertised to all clients
        // --------------------------
        // wl_compositor
        display_handle.create_global::<Climate, wlci::WlCompositor, ()>(4, ());
        display_handle.create_global::<Climate, xdg_wm_base::XdgWmBase, ()>(1, ());
        display_handle.create_global::<Climate, wl_seat::WlSeat, ()>(8, ());
        display_handle.create_global::<Climate, wl_subcompositor::WlSubcompositor, ()>(1, ());
        display_handle.create_global::<Climate, wl_output::WlOutput, ()>(4, ());
        display_handle.create_global::<Climate, zldv1::ZwpLinuxDmabufV1, ()>(3, ());
        display_handle.create_global::<Climate, wl_drm::WlDrm, ()>(2, ());
        display_handle.create_global::<Climate, wl_shell::WlShell, ()>(1, ());
        display_handle.create_global::<Climate, wl_shm::WlShm, ()>(1, ());

        return evman;
    }

    /// Helper method for registering the property id of a client
    ///
    /// We need to make an id for the client for our entity component set in
    /// the atmosphere. This method should be used when creating globals, so
    /// we can register the new client with the atmos
    ///
    /// Returns the id created
    pub fn register_new_client(
        &mut self,
        client_stream: std::os::unix::net::UnixStream,
    ) -> Result<ClientId> {
        let mut atmos = self.em_climate.c_atmos.lock().unwrap();
        // make a new client id
        let id = atmos.mint_client_id();
        // add our ClientData
        self.em_display.handle().insert_client(
            client_stream,
            Arc::new(ClientInfo {
                ci_id: id,
                ci_atmos: self.em_climate.c_atmos.clone(),
            }),
        )?;

        return Ok(id);
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
        // Add the libwayland internal descriptor
        fdw.add_fd(self.em_display.backend().poll_fd().as_raw_fd());
        // Add our libinput descriptor
        fdw.add_fd(self.em_climate.c_input.get_poll_fd());
        // Add the wayland socket itself
        fdw.add_fd(self.em_socket.as_raw_fd());
        // now register the fds we added
        fdw.register_events();

        // reset the timer before we start
        tm.reset();
        let mut needs_render = true;
        while needs_render || fdw.wait_for_events() {
            log::profiling!("starting loop");
            {
                let mut atmos = self.em_climate.c_atmos.lock().unwrap();
                // First thing to do is to dispatch libinput
                // It has time sensitive operations which need to take
                // place as soon as the fd is readable
                self.em_climate.c_input.dispatch(atmos.deref_mut());

                // TODO: fix frame timings to prevent the current state of
                // 3 frames of latency
                //
                // The input subsystem has batched the changes to the window
                // due to resizing, we need to send those changes now
                self.em_climate
                    .c_input
                    .update_from_eventloop(atmos.deref_mut());

                // Try to flip hemispheres to push our updates to vkcomp
                // If we can't recieve it, vkcomp isn't ready, and we should
                // continue processing wayland updates so the system
                // doesn't lag
                if atmos.is_changed() {
                    atmos.clear_changed();
                    needs_render = true;
                }
            }

            // Accept any new clients
            // Do this first to fill in their client data and initialize
            // atmos ids for each of them
            if let Some(client_stream) = self
                .em_socket
                .accept()
                .expect("Error reading wayland socket")
            {
                self.register_new_client(client_stream)
                    .expect("Could not register new client");
            }

            if needs_render {
                log::profiling!("trying to render frame");
                match self
                    .em_wm
                    .render_frame(self.em_climate.c_atmos.lock().unwrap().deref_mut())
                {
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

            // wait for the next event
            self.em_display
                .dispatch_clients(&mut self.em_climate)
                .unwrap();
            self.em_display
                .flush_clients()
                .expect("Could not flush wayland display");

            log::profiling!("EventManager: Blocking for max {} ms", tm.time_remaining());
        }
    }
}
