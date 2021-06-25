extern crate wayland_client;
use std::ops::Deref;
use wayland_client as wc;
use wc::protocol as wcp;

/// The wayland client struct.
///
/// This is the interaction point of the caller. It will be
/// used to connect to a compositor as a client, create
/// surfaces/buffers/etc, and register rendering callbacks.
struct Wayc {
    c_events: wc::EventQueue,
    c_disp: wc::Display,
    c_reg: wc::Main<wcp::wl_registry::WlRegistry>,
}

impl Wayc {
    pub fn new() -> Self {
        // connection to the wayland compositor
        let disp = wc::Display::connect_to_env().expect("Could not create a wl_display");
        let queue = disp.create_event_queue();
        // This is an annoying type dance we have to do: derefing the display gives us the &proxy
        // to the wl_display object, we deref a second time to get the Proxy<..>. Then we use Into
        // to turn it into the actual object (not just a proxy). Then we can actually use our
        // wl_display * to perform calls
        let wl_disp = disp.attach(queue.token());
        let registry = wl_disp.get_registry();

        let mut ret = Self {
            c_disp: disp,
            c_events: queue,
            c_reg: registry,
        };
        ret.register_globals(wl_disp);
        return ret;
    }

    fn register_globals(&mut self, wl_disp: wc::Attached<wcp::wl_display::WlDisplay>) {
        let gman = wc::GlobalManager::new(&wl_disp);

        for (_name, interface, version) in gman.list() {
            println!("Found global: {} ver {}", interface, version);
        }

        let wl_compositor = gman
            .instantiate_range::<wcp::wl_compositor::WlCompositor>(0, 4)
            .expect("Could not get the wl_compositor global");

        wl_compositor.quick_assign(move |_proxy, event, _| {
            match event {
                // All other requests are invalid
                _ => unimplemented!(),
            }
        });
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
