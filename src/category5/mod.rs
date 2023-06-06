// The Category 5 wayland compositor
//
// Austin Shafer - 2020
extern crate dakota as dak;
extern crate utils as cat5_utils;
extern crate wayland_protocols;
extern crate wayland_server as ws;

mod atmosphere;
mod input;
mod vkcomp;
mod ways;

use crate::category5::input::Input;
use atmosphere::Atmosphere;
use cat5_utils::{log, timing::*};
use utils::ClientId;
use vkcomp::wm::*;

use wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_v1 as zldv1;
use wayland_protocols::xdg::shell::server::*;
use ways::protocol::wl_drm::wl_drm;
use ws::protocol::{
    wl_compositor as wlci, wl_data_device_manager as wlddm, wl_output, wl_seat, wl_shell, wl_shm,
    wl_subcompositor,
};

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
    /// The vulkan renderer. It implements the draw logic,
    /// whereas WindowManager implements organizational logic
    c_dakota: dak::Dakota,
    /// The big database of all our properties
    c_atmos: Arc<Mutex<Atmosphere>>,
    /// The list of all output objects created for clients.
    ///
    /// We need this so that we can iterate through and signal size
    /// changes and the like.
    c_outputs: Vec<wl_output::WlOutput>,
    /// The input subsystem
    c_input: Input,
}

impl Climate {
    fn new() -> Self {
        Self {
            c_dakota: dak::Dakota::new().unwrap(),
            c_atmos: Arc::new(Mutex::new(Atmosphere::new())),
            c_outputs: Vec::new(),
            c_input: Input::new(),
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
        let mut state = Climate::new();
        let wm = WindowManager::new(
            &mut state.c_dakota,
            state.c_atmos.lock().unwrap().deref_mut(),
        );

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
        display_handle.create_global::<Climate, wlddm::WlDataDeviceManager, ()>(3, ());

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

    /// Handle Dakota notifying us that the display surface is out of date
    ///
    /// This is where we update the resolution and notify clients of the
    /// change
    fn handle_ood(&mut self) {
        let res = self.em_climate.c_dakota.get_resolution();
        {
            let mut atmos = self.em_climate.c_atmos.lock().unwrap();
            atmos.mark_changed();
            atmos.set_resolution(res.0, res.1);
        }
        self.em_climate.send_all_geometry();
        self.em_wm.handle_ood(&mut self.em_climate.c_dakota);
    }

    /// Helper to repeat Dakota's `dispatch_platform` until success
    ///
    /// This is needed for out of date handling.
    pub fn dispatch_dakota_platform(&mut self, mut timeout: Option<usize>) -> Result<()> {
        let mut first_loop = true;

        loop {
            if !first_loop {
                timeout = Some(0);
            }
            first_loop = false;

            // First handle input and platform changes
            match self
                .em_climate
                .c_dakota
                .dispatch_platform(&self.em_wm.wm_dakota_dom, timeout)
            {
                Ok(()) => {}
                Err(e) => {
                    if e.downcast_ref::<dak::DakotaError>() == Some(&dak::DakotaError::OUT_OF_DATE)
                    {
                        self.handle_ood();
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            };
            return Ok(());
        }
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
        // Add the libwayland internal descriptor
        self.em_climate
            .c_dakota
            .add_watch_fd(self.em_display.backend().poll_fd().as_raw_fd());
        // Add the wayland socket itself
        self.em_climate
            .c_dakota
            .add_watch_fd(self.em_socket.as_raw_fd());

        // reset the timer before we start
        tm.reset();
        let mut needs_render = true;
        loop {
            log::profiling!("starting loop");

            self.dispatch_dakota_platform(match needs_render {
                true => Some(0),
                false => None,
            })
            .expect("Dispatching Dakota platform handlers");

            {
                let mut atmos = self.em_climate.c_atmos.lock().unwrap();
                // First thing to do is to dispatch libinput
                // It has time sensitive operations which need to take
                // place as soon as the fd is readable
                // now go through each event
                for event in self.em_climate.c_dakota.drain_events() {
                    match &event {
                        // Don't print fd events since they happen constantly and
                        // flood the output
                        dak::Event::UserFdReadable => {}
                        dak::Event::WindowNeedsRedraw => needs_render = true,
                        dak::Event::WindowClosed { .. } => return,
                        e => {
                            log::error!("Category5: got Dakota event: {:?}", e);
                            self.em_climate
                                .c_input
                                .handle_input_event(atmos.deref_mut(), e);
                        }
                    }
                }

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
                let result = self.em_wm.render_frame(
                    &mut self.em_climate.c_dakota,
                    self.em_climate.c_atmos.lock().unwrap().deref_mut(),
                );

                match result {
                    Ok(()) => needs_render = false,
                    Err(e) => {
                        if let Some(err) = e.downcast_ref::<dak::DakotaError>() {
                            if *err == dak::DakotaError::NOT_READY
                                || *err == dak::DakotaError::TIMEOUT
                            {
                                // ignore the timeout, start our loop over
                                log::profiling!("Next frame isn't ready, continuing");
                            } else if *err == dak::DakotaError::OUT_OF_DATE {
                                self.handle_ood();
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
