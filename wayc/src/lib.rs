/// Wayc - A wayland client crate
///
/// This is a small standalone wayland helper crate. It wraps some of the details
/// of dealing with wayland objects so that you don't have to worry. It's designed for
/// being embedded into a ui toolkit, and can export objects to be used by rendering
/// engines like Thundr.
///
/// Austin Shafer - 2021
extern crate wayland_client;
use wayland_client as wc;
use wc::protocol as wcp;

extern crate anyhow;
pub use anyhow::{Context, Result};

// Now for the types we export
mod surface;
pub use surface::{Surface, SurfaceHandle};

mod role;
pub use role::Role;

mod buffer;
pub use buffer::{Buffer, BufferHandle};

/// The wayland client singleton.
///
/// This is the interaction point of the caller. It will be
/// used to connect to a compositor as a client, create
/// surfaces/buffers/etc, and register rendering callbacks.
pub struct Wayc {
    c_events: wc::EventQueue,
    c_disp: wc::Display,
    c_reg: wc::Main<wcp::wl_registry::WlRegistry>,
    c_compositor: wc::Main<wcp::wl_compositor::WlCompositor>,
}

impl Wayc {
    /// Create a new wayland connection.
    ///
    /// This will connec to the system compositor following normal env vars. It will
    /// then create a wl_registry, and initialize the necessary globals (including wl_compositor)
    pub fn new() -> Result<Self> {
        // connection to the wayland compositor
        let disp = wc::Display::connect_to_env().context("Could not create a wl_display")?;
        let queue = disp.create_event_queue();
        // This is an annoying type dance we have to do: derefing the display gives us the &proxy
        // to the wl_display object, we deref a second time to get the Proxy<..>. Then we use Into
        // to turn it into the actual object (not just a proxy). Then we can actually use our
        // wl_display * to perform calls
        let wl_disp = disp.attach(queue.token());
        let registry = wl_disp.get_registry();

        // Now register our globals
        let gman = wc::GlobalManager::new(&wl_disp);

        for (_name, interface, version) in gman.list() {
            println!("Found global: {} ver {}", interface, version);
        }

        let wl_compositor = gman
            .instantiate_range::<wcp::wl_compositor::WlCompositor>(0, 4)
            .context("Could not get the wl_compositor global")?;

        wl_compositor.quick_assign(move |_proxy, event, _| {
            match event {
                // All other requests are invalid
                _ => unimplemented!(),
            }
        });

        Ok(Self {
            c_disp: disp,
            c_events: queue,
            c_reg: registry,
            c_compositor: wl_compositor,
        })
    }

    pub fn create_surface(&mut self) -> SurfaceHandle {
        let wl_surf = self.c_compositor.create_surface();

        Surface::new(wl_surf)
    }

    pub fn dispatch(&mut self) {
        self.c_events
            .dispatch(&mut (), |_, _, _| {
                /* This closure will be called for every event received by an object not
                assigned to any Filter. If you plan to assign all your objects to Filter,
                the simplest thing to do is to assert this is never called. */
                unreachable!();
            })
            .expect("An error occurred during event dispatching!");
    }

    pub fn flush(&mut self) {
        self.c_disp.flush().unwrap();
    }
}
