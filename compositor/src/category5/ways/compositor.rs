// Wayland binding fun fun fun
//
//
// Austin Shafer - 2019

pub extern crate wayland_server as ws;
use ws::{Filter,Main,Resource};

use ws::protocol::{
    wl_compositor as wlci,
    wl_surface as wlsi,
    wl_shm,
};

use super::task::*;
use super::shm::*;
use super::surface::*;
use super::super::vkcomp::wm;

use std::time::Duration;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Sender,Receiver};

// A wayland compositor wrapper
//
// This class is point of first contact from higher levels in the stack.
// It roughly holds the wl_compositor/wl_display/wl_event_loop objects.
// There should be as little compositor logic in this as possible, most
// of the design decisions and actions should be taken by higher levels,
// using this API hides the unsafe bits.
//
// This is really the wayland compositor protocol object
#[allow(dead_code)]
pub struct Compositor {
    // A list of wayland client representations. These are the
    // currently connected clients.
    c_clients: Vec<RefCell<u32>>,
    // A list of surfaces which have been handed out to clients
    c_surfaces: Vec<Rc<RefCell<Surface>>>,
    c_wm_tx: Sender<wm::task::Task>,
    c_next_window_id: u64,
}

#[allow(dead_code)]
pub struct EventManager {
    // The wayland display object, this is the core
    // global singleton for libwayland
    em_display: ws::Display,
    em_wm_tx: Sender<wm::task::Task>,
    em_rx: Receiver<Task>,
}

impl Compositor {

    // wl_compositor interface create surface
    //
    //
    pub fn create_surface(&mut self, surf: Main<wlsi::WlSurface>) {
        println!("Creating surface");

        // Ask the window manage to create a new window
        // without contents
        self.c_next_window_id += 1;
        self.c_wm_tx.send(
            wm::task::Task::create_window(
                self.c_next_window_id, // ID of the new window
                0, 0, // position
                // No texture yet, it will be added by Surface
                64, 64, // window dimensions
            )
        ).unwrap();

        // create an entry in the surfaces list
        let id = self.c_next_window_id;
        let wm_tx = self.c_wm_tx.clone();

        let new_surface = Rc::new(RefCell::new(
            Surface::new(
                id,
                wm_tx,
                0, 0)
        ));
        let ns_clone = new_surface.clone();

        self.c_surfaces.push(new_surface.clone());

        // wayland_server takes care of creating the resource for
        // us, but we need to provide a function for it to call
        surf.quick_assign(move |s, r, _| {
            let mut nsurf = new_surface.borrow_mut();
            nsurf.handle_request(s, r);
        });
        // We have to manually assign a destructor, or else
        // Destroy request doesn't seem to proc
        surf.assign_destructor(Filter::new(
            move |_: Resource<wlsi::WlSurface>, _, _| {
                let mut nsurf = ns_clone.borrow_mut();
                nsurf.destroy();
            }
        ));
    }

    // Returns a new Compositor struct
    //
    // This creates a new wayland compositor, setting up all 
    // the needed resources for the struct. It will create a
    // wl_display, initialize a new socket, create the client/surface
    //  lists, and create a compositor global resource.
    //
    // This kicks off the global callback chain, starting with
    //    Compositor::bind_compositor_callback
    pub fn new(rx: Receiver<Task>, wm_tx: Sender<wm::task::Task>)
               -> Box<EventManager>
    {
        let mut display = ws::Display::new();
        display.add_socket_auto()
            .expect("Failed to add a socket to the wayland server");

        // Later moved into the closure
        let comp_cell = Rc::new(RefCell::new(
            Compositor {
                c_clients: Vec::new(),
                c_surfaces: Vec::new(),
                c_wm_tx: wm_tx.clone(),
                c_next_window_id: 1,
            }
        ));

        let mut evman = Box::new(EventManager {
            em_display: display,
            em_wm_tx: wm_tx,
            em_rx: rx,
        });

        evman.create_surface_global(comp_cell);
        evman.create_shm_global();

        return evman;
    }
}

impl EventManager {
    
    fn create_surface_global(&mut self,
                             comp_cell: Rc<RefCell<Compositor>>) {
        // create interface for our compositor
        // this global is independent of any one client, and
        // will be the first thing they bind
        self.em_display.create_global::<wlci::WlCompositor, _>(
            3, // version
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

    fn create_shm_global(&mut self) {
        // create interface for our compositor
        // this global is independent of any one client, and
        // will be the first thing they bind
        self.em_display.create_global::<wl_shm::WlShm, _>(
            1, // version
            Filter::new(
                // This closure will be called when wl_compositor_interface
                // is bound. args are (resource, version)
                move |(r, _): (ws::Main<wl_shm::WlShm>, u32), _, _| {
                    r.quick_assign(move |p, r, _| {
                        shm_handle_request(r, p);
                    });
                    r.format(wl_shm::Format::Xrgb8888);
                }
            ),
        );
    }

    pub fn worker_thread(&mut self) {
        loop {
            // wait for the next event
            self.em_display
                .dispatch(Duration::from_millis(0), &mut ())
                .unwrap();
            self.em_display.flush_clients(&mut ());

            //let task = self.rx.recv().unwrap();
            //self.process_task(&task);
        }
    }
}
