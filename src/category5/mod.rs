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
use atmosphere::{Atmosphere, ClientId};
use cat5_utils::{log, Result};
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

// The category5 compositor
#[allow(dead_code)]
pub struct Category5 {
    c5_ev: EventManager,
}

impl Category5 {
    // This is a cooler way of saying new
    // I got bored of writing new constantly
    pub fn spin() -> Category5 {
        Category5 {
            // Get the wayland compositor
            // Note that the wayland compositor + vulkan renderer
            // is the complete compositor
            c5_ev: EventManager::new(),
        }
    }

    // This is the main loop of the entire system
    // We just wait for the other threads
    pub fn run_forever(&mut self) {
        self.c5_ev.worker_thread();
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
    /// This is our toplevel Dakota UI toolkit handle
    c_dakota: dak::Dakota,
    /// This is the virtual surface that we lay out a desktop on
    /// and present portions of.
    c_virtual_output: dak::VirtualOutput,
    /// The DRM format modifiers supported by the primary GPU
    c_primary_render_mods: Vec<u64>,
    /// This is our scene, a layout tree of the Dakota Elements which
    /// correspond to our Wayland surfaces.
    c_scene: dak::Scene,
    /// This is a database containing tables of properties for Wayland
    /// surfaces and clients.
    c_atmos: Arc<Mutex<Atmosphere>>,
    /// The list of all output objects created for clients.
    ///
    /// We need this so that we can iterate through and signal size
    /// changes and the like.
    // TODO: make this a Component for OutputId
    c_outputs: Vec<wl_output::WlOutput>,
    /// The input subsystem
    c_input: Input,
}

impl Climate {
    fn new() -> Self {
        let mut dakota = dak::Dakota::new().expect("Could not create dakota instance");

        let mut virtual_output = dakota
            .create_virtual_output()
            .expect("Failed to create Dakota Virtual Output Surface");

        // Set a default resolution. The window manager will update this
        // as Outputs are added
        virtual_output.set_size((128, 128));

        let scene = dakota
            .create_scene(&virtual_output)
            .expect("Could not create scene");

        Self {
            c_atmos: Arc::new(Mutex::new(Atmosphere::new(&scene))),
            c_primary_render_mods: dakota.get_supported_drm_render_modifiers(),
            c_dakota: dakota,
            c_virtual_output: virtual_output,
            c_scene: scene,
            c_outputs: Vec::with_capacity(1),
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
    _ci_atmos: Arc<Mutex<Atmosphere>>,
}

impl ws::backend::ClientData for ClientInfo {
    fn initialized(&self, _client_id: ws::backend::ClientId) {}
    fn disconnected(
        &self,
        _client_id: ws::backend::ClientId,
        _reason: ws::backend::DisconnectReason,
    ) {
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
    pub fn new() -> EventManager {
        let display = ws::Display::new().expect("Could not create wayland display");
        let display_handle = display.handle();

        // Our big state holder for wayland-rs
        let mut state = Climate::new();
        let wm = WindowManager::new(
            &mut state.c_dakota,
            &mut state.c_virtual_output,
            &mut state.c_scene,
            state.c_atmos.lock().unwrap().deref_mut(),
        )
        .expect("Could not create Window Manager");

        let evman = EventManager {
            em_wm: wm,
            em_climate: state,
            em_display: display,
            em_socket: ws::ListeningSocket::bind_auto("wayland", 0..9)
                .expect("Could not create wayland socket"),
        };

        // Register our global interfaces that will be advertised to all clients
        // --------------------------
        // wl_compositor
        display_handle.create_global::<Climate, wlci::WlCompositor, ()>(5, ());
        display_handle.create_global::<Climate, xdg_wm_base::XdgWmBase, ()>(1, ());
        display_handle.create_global::<Climate, wl_seat::WlSeat, ()>(8, ());
        display_handle.create_global::<Climate, wl_subcompositor::WlSubcompositor, ()>(1, ());
        display_handle.create_global::<Climate, wl_output::WlOutput, ()>(4, ());
        if evman.em_climate.c_atmos.lock().unwrap().get_drm_dev() != (0, 0) {
            log::debug!("No DRM device detected, not advertising DRM-based interfaces");
            display_handle.create_global::<Climate, zldv1::ZwpLinuxDmabufV1, ()>(3, ());
            display_handle.create_global::<Climate, wl_drm::WlDrm, ()>(2, ());
        }
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
                ci_id: id.clone(),
                _ci_atmos: self.em_climate.c_atmos.clone(),
            }),
        )?;

        return Ok(id);
    }

    /// Each subsystem has a function that implements its main
    /// loop. This is that function
    pub fn worker_thread(&mut self) {
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

        loop {
            log::debug!("starting loop");

            self.em_climate
                .c_dakota
                .dispatch(None)
                .expect("Dispatching Dakota platform handlers");
            log::debug!("dispatch_platform done");

            log::debug!("begin event handling");
            // First thing to do is to dispatch libinput
            // It has time sensitive operations which need to take
            // place as soon as the fd is readable
            // now go through each event
            for event in self.em_climate.c_dakota.drain_events() {
                match &event {
                    // Don't print fd events since they happen constantly and
                    // flood the output
                    dak::GlobalEvent::UserFdReadable => {}
                    // Exit gracefully if quit
                    dak::GlobalEvent::Quit => return,
                }
            }
            log::debug!("Global handling done");

            while let Some(ev) = self.em_climate.c_virtual_output.pop_event() {
                match &ev {
                    e => {
                        log::debug!("Category5: got Dakota PlatformEvent: {:?}", e);
                        self.em_climate.c_input.handle_input_event(
                            self.em_climate.c_atmos.lock().unwrap().deref_mut(),
                            e,
                        );
                    }
                }
            }
            log::debug!("Platform handling done");

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

            // Handle any available wayland events.
            // We should do this before rendering so that any updates are reflected
            // immediately.
            log::debug!("dispatching wayland");
            self.em_display
                .dispatch_clients(&mut self.em_climate)
                .unwrap();

            let mut atmos = self.em_climate.c_atmos.lock().unwrap();
            self.em_wm
                .dispatch_drawing(
                    &mut self.em_climate.c_virtual_output,
                    &mut self.em_climate.c_scene,
                    &mut atmos,
                )
                .unwrap();
            atmos.clear_changed();
            log::debug!("Output handling done");

            // Flush any wayland events we sent here
            // The rendering code will send the wayland frame notifications, which
            // have been queued but not yet flushed to the wayland socket.
            log::debug!("flushing wayland");
            self.em_display
                .flush_clients()
                .expect("Could not flush wayland display");
        }
    }
}
