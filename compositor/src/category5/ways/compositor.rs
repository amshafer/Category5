// Wayland compositor singleton
//
// This is the "top" of the wayland heirarchy,
// it is the initiating module of the wayland
// protocols
//
// Austin Shafer - 2019
extern crate nix;
pub extern crate wayland_server as ws;
use ws::{Filter,Main,Resource};

use ws::protocol::{
    wl_compositor as wlci,
    wl_surface as wlsi,
    wl_shm,
    wl_shell,
};

use crate::category5::utils::{
    timing::*, logging::LogLevel, atmosphere::*
};
use crate::log;
use crate::category5::input::{Input, event::*};
use super::{
    shm::*,
    surface::*,
    wl_shell::wl_shell_handle_request,
    xdg_shell::xdg_wm_base_handle_request,
    linux_dmabuf::*,
};
use super::protocol::{
    xdg_shell::xdg_wm_base,
    linux_dmabuf::zwp_linux_dmabuf_v1 as zldv1,
};

use nix::sys::event::*;
use std::time::Duration;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Sender,Receiver};
use std::ops::Deref;
use std::os::unix::io::RawFd;

// A wayland compositor wrapper
//
// This is the singleton of the wayland subsystem. It holds
// all of the high level state and is passed and reference
// counted to all of the protocol state objects. These objects
// will perform their operations and update this state if needed.
//
// Obviously anything that can be kept in protocol objects should,
// for sake of parallelism.
#[allow(dead_code)]
pub struct Compositor {
    c_atmos: Rc<RefCell<Atmosphere>>,
    // counter for the next window id to hand out
    c_next_window_id: u32,
}

// The event manager
//
// This class the launching point of the wayland stack. It
// is used by category5 to dispatch handling and listen
// on the wayland fds. It also owns the wayland-rs top
// level object in em_display
#[allow(dead_code)]
pub struct EventManager {
    em_atmos: Rc<RefCell<Atmosphere>>,
    // The wayland display object, this is the core
    // global singleton for libwayland
    em_display: ws::Display,
    // The input subsystem
    //
    // This is not in its own thread since it generates a
    // huge amount of updates, which performs poorly with
    // channel-based message passing.
    em_input: Input,
    // How much the mouse has moved in this frame
    // aggregates input pointer events
    em_pointer_dx: f64,
    em_pointer_dy: f64,
}

// Helper for creating an empty KEvent for kqueue
// This is just for placeholders when we need
// an initialized kevent
fn empty_kevent() -> KEvent {
   read_fd_kevent(0)
}

// Helper for creating a kevent for reading an fd
fn read_fd_kevent(fd: RawFd) -> KEvent {
    KEvent::new(fd as usize,
                EventFilter::EVFILT_READ,
                EventFlag::EV_ADD,
                FilterFlag::all(),
                0, 0)
}

impl Compositor {

    // wl_compositor interface create surface
    //
    // This request creates a new wl_surface and
    // hooks up our surface handler. See the surface
    // module
    pub fn create_surface(&mut self, surf: Main<wlsi::WlSurface>) {
        // Ask the window manage to create a new window
        // without contents
        self.c_next_window_id += 1;

        // create an entry in the surfaces list
        let id = self.c_next_window_id;

        // Create a reference counted object
        // in charge of this new surface
        let new_surface = Rc::new(RefCell::new(
            Surface::new(
                self.c_atmos.clone(),
                id,
                0, 0)
        ));
        // This clone will be passed to the surface handler
        let ns_clone = new_surface.clone();
        // Track this surface in the compositor state
        self.c_atmos.borrow_mut().add_window_id(id);

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
        surf.assign_destructor(Filter::new(
            move |_: Resource<wlsi::WlSurface>, _, _| {
                let mut nsurf = ns_clone.borrow_mut();
                nsurf.destroy();
            }
        ));
    }
}

impl EventManager {
    // Returns a new Compositor struct
    //    (okay well really an EventManager)
    //
    // This creates a new wayland compositor, setting up all 
    // the needed resources for the struct. It will create a
    // wl_display, initialize a new socket, create the client/surface
    //  lists, and create a compositor global resource.
    //
    // This kicks off the global callback chain, starting with
    //    Compositor::bind_compositor_callback
    pub fn new(tx: Sender<Box<Hemisphere>>,
               rx: Receiver<Box<Hemisphere>>)
               -> Box<EventManager>
    {
        let mut display = ws::Display::new();
        display.add_socket_auto()
            .expect("Failed to add a socket to the wayland server");

        // Do some teraforming and generate an atmosphere
        let atmos = Rc::new(RefCell::new(Atmosphere::new(tx, rx)));

        // Later moved into the closure
        let comp_cell = Rc::new(RefCell::new(
            Compositor {
                c_atmos: atmos.clone(),
                c_next_window_id: 1,
            }
        ));

        let mut evman = Box::new(EventManager {
            em_atmos: atmos,
            em_display: display,
            em_input: Input::new(),
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

        return evman;
    }

    // Create a new global object advertising the wl_surface interface
    //
    // In wayland we create global objects which tell the client
    // what protocols we implement. Each of these methods initializes
    // one such global
    fn create_compositor_global(&mut self,
                                comp_cell: Rc<RefCell<Compositor>>) {
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
                            wlci::Request::CreateSurface { id } =>
                                comp.create_surface(id),
                            // All other requests are invalid
                            _ => unimplemented!(),
                        }
                    });
                }
            ),
        );
    }

    // Create the shared memory globals
    //
    // This creates the wl_shm interface. It seems that
    // wayland-rs does not handle this interface for us
    // like the system library does, so we create it here
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
                }
            ),
        );
    }

    // Initialize the wl_shell interface
    //
    // the wl_shell interface handles the desktop window
    // lifecycle. It handles the type of window and its position
    fn create_wl_shell_global(&mut self) {
        self.em_display.create_global::<wl_shell::WlShell, _>(
            1, // version
            Filter::new(
                // This filter is called when wl_shell_interface is bound
                move |(r, _): (ws::Main<wl_shell::WlShell>, u32), _, _| {
                    r.quick_assign(move |p, r, _| {
                        wl_shell_handle_request(r, p);
                    });
                }
            ),
        );
    }

    // Initialize the wl_shell interface
    //
    // the wl_shell interface handles the desktop window
    // lifecycle. It handles the type of window and its position
    fn create_xdg_shell_global(&mut self) {
        self.em_display.create_global::<xdg_wm_base::XdgWmBase, _>(
            1, // version
            Filter::new(
                // This filter is called when xdg_shell_interface is bound
                move |(res, _): (ws::Main<xdg_wm_base::XdgWmBase>, u32), _, _| {
                    res.quick_assign(move |x, r, _| {
                        xdg_wm_base_handle_request(r, x);
                    });
                }
            ),
        );
    }

    // Initialize the linux_dmabuf interface
    //
    // This interface provides a way to import GPU buffers from
    // clients, avoiding lots of copies. It passes dmabuf fds.
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
                }
            ),
        );
    }

    // Each subsystem has a function that implements its main
    // loop. This is that function
    pub fn worker_thread(&mut self) {
        // We want to track every 15ms. This is a little less than
        // once per 60fps frame. It doesn't have to be exact, but
        // we need to send certain updates to vkcomp roughly once per
        // frame
        let mut tm = TimingManager::new(4);

        // wayland-rs will not do blocking for us,
        // so we need to use kqueue. This is the
        // same approach as used by the input
        // subsystem.
        let ways_fd = self.em_display.get_poll_fd();
        let input_fd = self.em_input.get_poll_fd();

        // Create a new kqueue
        let kq = kqueue().expect("Could not create kqueue");

        // Create read events for ways and input
        // When registered, these will tell kqueue to notify
        // use when the wayland or libinput fds are readable
        let kev_ways = read_fd_kevent(ways_fd);
        let kev_input = read_fd_kevent(input_fd);

        // Register our kevent with the kqueue to receive updates
        kevent(kq, vec![kev_ways, kev_input].as_slice(), &mut [], 0)
            .expect("Could not register watch event with kqueue");

        // List of triggered events
        // one for wayland and one for input
        let mut evlist = vec![empty_kevent(), empty_kevent()];

        // reset the timer before we start
        tm.reset();
        while kevent(kq, &[], evlist.as_mut_slice(),
                     // timeout after 15 ms (16 is the ms per frame at 60fps)
                     tm.time_remaining()).is_ok() {
            // First thing to do is to dispatch libinput
            // It has time sensitive operations which need to take
            // place as soon as the fd is readable
            self.em_input.dispatch();
            while let Some(iev) = self.em_input.next_available() {
                match iev {
                    InputEvent::pointer_move(m) => {
                        // Coalesce movement so we only send one message
                        // to vkcomp per frame for efficiency
                        self.em_pointer_dx += m.pm_dx;
                        self.em_pointer_dy += m.pm_dy;
                    },
                    InputEvent::left_click(_) => {
                        // TODO: start grab
                    }
                }
            }

            // TODO: This might not be the most accurate
            if tm.is_overdue() {
                // reset our timer
                tm.reset();
                // Flip hemispheres to push our updates to vkcomp
                // this must be terrible for the local fauna
                //
                // This is a synchronization point. It will block
                self.em_atmos.borrow_mut().flip_hemispheres();
                // it has been roughly one frame, so fire the frame callbacks
                // so clients can draw
                self.em_atmos.borrow_mut().signal_frame_callbacks();
            }

            // wait for the next event
            self.em_display
                .dispatch(Duration::from_millis(0), &mut ())
                .unwrap();
            self.em_display.flush_clients(&mut ());

            //println!("EventManager: Blocking for max {} ms", tm.time_remaining());
        }
    }
}
