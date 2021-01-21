// Wayland compositor singleton
//
// This is the "top" of the wayland heirarchy,
// it is the initiating module of the wayland
// protocols
//
// Austin Shafer - 2019
extern crate input;
pub extern crate wayland_server as ws;
use ws::{Filter, Main, Resource};

use ws::protocol::{
    wl_compositor as wlci, wl_data_device_manager as wlddm, wl_output, wl_seat, wl_shell, wl_shm,
    wl_subcompositor as wlsc, wl_surface as wlsi,
};

extern crate utils as cat5_utils;
use super::protocol::{linux_dmabuf::zwp_linux_dmabuf_v1 as zldv1, xdg_shell::xdg_wm_base};
use super::seat::Seat;
use super::utils;
use super::{
    linux_dmabuf::*, shm::*, surface::*, wl_output::wl_output_broadcast, wl_region,
    wl_shell::wl_shell_handle_request, wl_subcompositor::wl_subcompositor_handle_request,
    xdg_shell::xdg_wm_base_handle_request,
};
use crate::category5::atmosphere::{Atmosphere, Hemisphere};
use crate::category5::input::Input;
use cat5_utils::{fdwatch::FdWatch, log, timing::*};

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

/// A wayland compositor wrapper
///
/// This is the singleton of the wayland subsystem. It holds
/// all of the high level state and is passed and reference
/// counted to all of the protocol state objects. These objects
/// will perform their operations and update this state if needed.
///
/// Obviously anything that can be kept in protocol objects should,
/// for sake of parallelism.
#[allow(dead_code)]
pub struct Compositor {
    c_atmos: Rc<RefCell<Atmosphere>>,
}

/// The event manager
///
/// This class the launching point of the wayland stack. It
/// is used by category5 to dispatch handling and listen
/// on the wayland fds. It also owns the wayland-rs top
/// level object in em_display
#[allow(dead_code)]
pub struct EventManager {
    em_atmos: Rc<RefCell<Atmosphere>>,
    /// The wayland display object, this is the core
    /// global singleton for libwayland
    em_display: ws::Display,
    /// The input subsystem
    ///
    /// This is not in its own thread since it generates a
    /// huge amount of updates, which performs poorly with
    /// channel-based message passing.
    em_input: Rc<RefCell<Input>>,
    /// How much the mouse has moved in this frame
    /// aggregates input pointer events
    em_pointer_dx: f64,
    em_pointer_dy: f64,
}

impl Compositor {
    /// wl_compositor interface create surface
    ///
    /// This request creates a new wl_surface and
    /// hooks up our surface handler. See the surface
    /// module
    pub fn create_surface(&mut self, surf: Main<wlsi::WlSurface>) {
        let client = utils::get_id_from_client(
            self.c_atmos.clone(),
            surf.as_ref()
                .client()
                .expect("client for this surface seems to have disappeared"),
        );
        let id = self.c_atmos.borrow_mut().mint_window_id(client);
        log::debug!("Creating new surface {:?}", id);

        // Create a reference counted object
        // in charge of this new surface
        let new_surface = Rc::new(RefCell::new(Surface::new(
            self.c_atmos.clone(),
            surf.clone(),
            id,
        )));
        // Add the new surface to the atmosphere
        {
            let mut atmos = self.c_atmos.borrow_mut();
            atmos.add_surface(id, new_surface.clone());
            // get the Resource<WlSurface>, turn it into a &WlSurface
            atmos.set_wl_surface(id, surf.as_ref().clone().into());
        }

        // This clone will be passed to the surface handler
        let ns_clone = new_surface.clone();

        // wayland_server takes care of creating the resource for
        // us, but we need to provide a function for it to call
        surf.quick_assign(move |s, r, _| {
            // Get a reference to the Surface
            let mut nsurf = new_surface.borrow_mut();
            nsurf.handle_request(s, r);
        });

        // Add the new surface to the userdata so other
        // protocols can see it
        surf.as_ref().user_data().set(|| ns_clone.clone());

        // We have to manually assign a destructor, or else
        // Destroy request doesn't seem to proc
        let destroy_atmos = self.c_atmos.clone();
        surf.assign_destructor(Filter::new(move |_: Resource<wlsi::WlSurface>, _, _| {
            let mut nsurf = ns_clone.borrow_mut();
            let mut atmos = destroy_atmos.borrow_mut();
            nsurf.destroy(&mut atmos);
        }));
    }
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
    pub fn new(tx: Sender<Box<Hemisphere>>, rx: Receiver<Box<Hemisphere>>) -> Box<EventManager> {
        let mut display = ws::Display::new();
        display
            .add_socket_auto()
            .expect("Failed to add a socket to the wayland server");

        // Do some teraforming and generate an atmosphere
        let atmos = Rc::new(RefCell::new(Atmosphere::new(tx, rx)));

        // Later moved into the closure
        let comp_cell = Rc::new(RefCell::new(Compositor {
            c_atmos: atmos.clone(),
        }));

        let mut evman = Box::new(EventManager {
            em_atmos: atmos.clone(),
            em_display: display,
            em_input: Rc::new(RefCell::new(Input::new(atmos))),
            em_pointer_dx: 0.0,
            em_pointer_dy: 0.0,
        });

        // Register our global interfaces that
        // will be advertised to all clients
        evman.create_compositor_global(comp_cell);
        evman.create_shm_global();
        evman.create_wl_shell_global();
        evman.create_xdg_shell_global();
        evman.create_linux_dmabuf_global();
        evman.create_wl_seat_global();
        evman.create_wl_output_global();
        evman.create_wl_subcompositor_global();
        evman.create_wl_data_device_manager_global();

        return evman;
    }

    /// Create a new global object advertising the wl_surface interface
    ///
    /// In wayland we create global objects which tell the client
    /// what protocols we implement. Each of these methods initializes
    /// one such global
    fn create_compositor_global(&mut self, comp_cell: Rc<RefCell<Compositor>>) {
        // create interface for our compositor
        // this global is independent of any one client, and
        // will be the first thing they bind
        self.em_display.create_global::<wlci::WlCompositor, _>(
            4, // version
            Filter::new(
                // This closure will be called when wl_compositor_interface
                // is bound. args are (resource, version)
                move |(r, _): (ws::Main<wlci::WlCompositor>, u32), _, _| {
                    // We need to create a filter that will be called when this
                    // object is requested. This closure just maps requests
                    // to their handling functions
                    let comp_clone = comp_cell.clone();
                    r.quick_assign(move |_proxy, request, _| {
                        let mut comp = comp_clone.borrow_mut();
                        match request {
                            wlci::Request::CreateSurface { id } => comp.create_surface(id),
                            wlci::Request::CreateRegion { id } => wl_region::register_new(id),
                            // All other requests are invalid
                            _ => unimplemented!(),
                        }
                    });
                },
            ),
        );
    }

    /// Create the shared memory globals
    ///
    /// This creates the wl_shm interface. It seems that
    /// wayland-rs does not handle this interface for us
    /// like the system library does, so we create it here
    fn create_shm_global(&mut self) {
        self.em_display.create_global::<wl_shm::WlShm, _>(
            1, // version
            Filter::new(
                // This closure will be called when wl_shm_interface
                // is bound. args are (resource, version)
                move |(r, _): (ws::Main<wl_shm::WlShm>, u32), _, _| {
                    r.quick_assign(move |shm, r, _| {
                        // clone the WlShm so it doesn't get dropped
                        // prematurely
                        shm_handle_request(r, shm.deref().clone());
                    });
                    r.format(wl_shm::Format::Xrgb8888);
                },
            ),
        );
    }

    /// Initialize the wl_shell interface
    ///
    /// the wl_shell interface handles the desktop window
    /// lifecycle. It handles the type of window and its position
    fn create_wl_shell_global(&mut self) {
        self.em_display.create_global::<wl_shell::WlShell, _>(
            1, // version
            Filter::new(
                // This filter is called when wl_shell_interface is bound
                move |(r, _): (ws::Main<wl_shell::WlShell>, u32), _, _| {
                    r.quick_assign(move |p, r, _| {
                        wl_shell_handle_request(r, p);
                    });
                },
            ),
        );
    }

    /// Initialize the wl_shell interface
    ///
    /// the wl_shell interface handles the desktop window
    /// lifecycle. It handles the type of window and its position
    fn create_xdg_shell_global(&mut self) {
        self.em_display.create_global::<xdg_wm_base::XdgWmBase, _>(
            1, // version
            Filter::new(
                // This filter is called when xdg_shell_interface is bound
                move |(res, _): (ws::Main<xdg_wm_base::XdgWmBase>, u32), _, _| {
                    res.quick_assign(move |x, r, _| {
                        xdg_wm_base_handle_request(r, x);
                    });
                },
            ),
        );
    }

    /// Initialize the linux_dmabuf interface
    ///
    /// This interface provides a way to import GPU buffers from
    /// clients, avoiding lots of copies. It passes dmabuf fds.
    fn create_linux_dmabuf_global(&mut self) {
        self.em_display.create_global::<zldv1::ZwpLinuxDmabufV1, _>(
            3, // version
            Filter::new(
                // This filter is called when xdg_shell_interface is bound
                move |(res, _): (ws::Main<zldv1::ZwpLinuxDmabufV1>, u32), _, _| {
                    // We need to broadcast supported formats
                    linux_dmabuf_setup(res.clone());

                    // now we can handle the event
                    res.quick_assign(move |l, r, _| {
                        linux_dmabuf_handle_request(r, l);
                    });
                },
            ),
        );
    }

    /// Initialize the wl_seat interface
    ///
    /// A wl_seat represents a group of input devices that a human
    /// is sitting in front of. This provisions the input interfaces
    fn create_wl_seat_global(&mut self) {
        // for some reason we need to do two clones to make the lifetime
        // inference happy with the closures below
        let atmos_rc = self.em_atmos.clone();
        let input_sys = self.em_input.clone();

        self.em_display.create_global::<wl_seat::WlSeat, _>(
            5, // version
            Filter::new(move |(res, _): (ws::Main<wl_seat::WlSeat>, u32), _, _| {
                // as_ref turns the Main into a Resource
                let client = res.as_ref().client().unwrap();
                // get the id representing this client in the atmos
                let id = utils::get_id_from_client(atmos_rc.clone(), client);

                // check if a seat exists and add this to it
                // add a new seat to this client
                let mut atmos = atmos_rc.borrow_mut();
                let seat = match atmos.get_seat_from_client_id(id) {
                    Some(seat) => {
                        // TODO: add wl_seat to this Seat
                        seat
                    }
                    None => {
                        let seat =
                            Rc::new(RefCell::new(Seat::new(input_sys.clone(), id, res.clone())));
                        atmos.add_seat(id, seat.clone());
                        seat
                    }
                };
                // now we can handle the event
                res.quick_assign(move |s, r, _| {
                    seat.borrow_mut().handle_request(r, s);
                });
            }),
        );
    }

    /// Initialize the wl_output interface
    ///
    /// This doesn't do much of anything except advertise what displays
    /// are available for the clients
    fn create_wl_output_global(&mut self) {
        // for some reason we need to do two clones to make the lifetime
        // inference happy with the closures below
        let atmos = self.em_atmos.clone();

        self.em_display.create_global::<wl_output::WlOutput, _>(
            3, // version
            Filter::new(
                move |(res, _): (ws::Main<wl_output::WlOutput>, u32), _, _| {
                    wl_output_broadcast(atmos.clone(), res);
                },
            ),
        );
    }

    /// Initialize the wl_subcompositor interface
    ///
    /// In certain scenarios it is desired to let the compositor do the compositing
    /// for the application. This interface allows the app to arbitrarily nest
    /// subsurfaces
    fn create_wl_subcompositor_global(&mut self) {
        self.em_display.create_global::<wlsc::WlSubcompositor, _>(
            1, // version
            Filter::new(
                move |(res, _): (ws::Main<wlsc::WlSubcompositor>, u32), _, _| {
                    res.quick_assign(move |s, r, _| {
                        wl_subcompositor_handle_request(r, s);
                    });
                },
            ),
        );
    }

    /// This is a manager for creating cut/pase & drag/drop objects.
    ///
    /// All apps that use such functionality will bind this and mint the objects they need
    fn create_wl_data_device_manager_global(&mut self) {
        self.em_display
            .create_global::<wlddm::WlDataDeviceManager, _>(
                1, // version
                Filter::new(
                    move |(res, _): (ws::Main<wlddm::WlDataDeviceManager>, u32), _, _| {
                        res.quick_assign(move |_, _, _| {
                            // TODO:
                        });
                    },
                ),
            );
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
        fdw.add_fd(self.em_input.borrow_mut().get_poll_fd());
        // now register the fds we added
        fdw.register_events();

        // Do we need to send our hemisphere
        let mut needs_send = true;
        // do we need to send frame callbacks
        let mut needs_frame = true;

        // reset the timer before we start
        tm.reset();
        while fdw.wait_for_events(tm.time_remaining()) {
            log::profiling!("starting loop");
            // First thing to do is to dispatch libinput
            // It has time sensitive operations which need to take
            // place as soon as the fd is readable
            self.em_input.borrow_mut().dispatch();

            // TODO: This might not be the most accurate
            if tm.is_overdue() {
                log::profiling!("timer out");
                // TODO: fix frame timings to prevent the current state of
                // 3 frames of latency
                //
                // The input subsystem has batched the changes to the window
                // due to resizing, we need to send those changes now
                self.em_input.borrow_mut().update_shell();

                if needs_frame {
                    needs_frame = false;
                    // it has been roughly one frame, so fire the frame callbacks
                    // so clients can draw
                    self.em_atmos.borrow_mut().signal_frame_callbacks();
                }

                // Try to flip hemispheres to push our updates to vkcomp
                // First we need to send our hemisphere, then we can
                // try to recv it later. If we can't recieve it, then
                // continue processing wayland updates so the system
                // doesn't lag
                if self.em_atmos.borrow_mut().is_changed() {
                    log::profiling!("finished frame");
                    if needs_send {
                        needs_send = false;
                        log::debug!("_____________________________ SENT HEMI");
                        self.em_atmos.borrow_mut().send_hemisphere();
                    }
                    if self.em_atmos.borrow_mut().recv_hemisphere() {
                        log::debug!("_____________________________ RECV HEMI");
                        // reset our timer
                        tm.reset();
                        needs_send = true;
                        needs_frame = true;
                    }
                } else {
                    // The atmosphere was not changed,
                    tm.reset();
                    needs_frame = true;
                    needs_send = true;
                }
            }

            // wait for the next event
            self.em_display
                .dispatch(Duration::from_millis(0), &mut ())
                .unwrap();
            self.em_display.flush_clients(&mut ());

            log::profiling!("EventManager: Blocking for max {} ms", tm.time_remaining());
        }
    }
}
