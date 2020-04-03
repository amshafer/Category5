// Wayland binding fun fun fun
//
//
// Austin Shafer - 2019
use super::wayland_bindings::*;
use super::wayland_safe::*;
use super::wl_shell;
use super::task::*;

use super::surface::{Surface, SURFACE_INTERFACE};

use std::cell::RefCell;
use std::slice;
use super::super::vkcomp::wm;
use std::os::raw::c_void;

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
pub struct Compositor {
    // A list of wayland client representations. These are the
    // currently connected clients.
    c_clients: Vec<RefCell<u32>>,
    // A list of surfaces which have been handed out to clients
    c_surfaces: Vec<RefCell<Surface>>,
    c_wm_tx: Sender<wm::task::Task>,
    c_next_window_id: u64,
}

pub struct EventManager {
    // The compositor interface
    em_compositor: RefCell<Compositor>,
    // The wayland display object, this is the core
    // global singleton for libwayland
    em_display: WLDisplay,
    // This struct holds the event loop, as we want to abstract
    // the unsafe code for flushing clients and waiting for events
    em_event_loop: WLEventLoop,
    em_event_loop_fd: i32,
    em_wm_tx: Sender<wm::task::Task>,
    em_rx: Receiver<Task>,
}

// wlci - wl compositor interface
//
// our implementation of wl_compositor_interface. The compositor singleton
// kicks off the window creation process by creating a surface
static COMPOSITOR_INTERFACE: wl_compositor_interface =
    wl_compositor_interface {
        // called asynchronously by the client when they
        // want to request a surface
        create_surface: Some(Compositor::create_surface),
        // wl_region represent an opaque input region in the surface
        create_region: Some(Compositor::create_region),
    };

impl Compositor {
    // Callback for our implementation of the wl_compositor inferface
    //
    // When the client binds a wl_compositor interface this will be called,
    // and we can set our implementation so that surfaces/regions can be
    // created.
    // the data arg is added as the private data for the implementation.
    pub extern "C" fn bind_compositor_callback(
        client: *mut wl_client,
        data: *mut c_void,
        version: u32,
        id: u32)
    {
        let comp_ref = unsafe {
            // Get a slice of one Compositor, then grab a ref
            // to the first one
            &mut slice::from_raw_parts_mut(
                data as *mut RefCell<Compositor>, 1)[0]
        };
        println!("Binding the compositor interface");

        let res = ws_resource_create!(
            WLClient::from_ptr(client),
            wl_compositor_interface,
            1, id
        );
        ws_resource_set_implementation(
            res,
            &COMPOSITOR_INTERFACE,
            Some(comp_ref),
            None
        );
    }

    // wl_compositor interface create surface
    //
    // Here we create a resource for the new surface, specifying
    // our surface interface. It will be called next when the surface
    // is bound.
    pub extern "C" fn create_surface(client: *mut wl_client,
                                     resource: *mut wl_resource,
                                     id: u32)
    {
        println!("Creating surface");
        // get the compositor from the resource
        let mut comp = get_userdata_of_type!(
            resource,
            Compositor
        ).unwrap();

        // first get a new resource to represent our surface
        let res = ws_resource_create!(WLClient::from_ptr(client),
                                      wl_surface_interface, 3, id);

        // Ask the window manage to create a new window
        // without contents
        comp.c_next_window_id += 1;
        comp.c_wm_tx.send(
            wm::task::Task::create_window(
                comp.c_next_window_id, // ID of the new window
                0, 0, // position
                // No texture yet, it will be added by Surface
                64, 64, // window dimensions
            )
        ).unwrap();

        // create an entry in the surfaces list
        let id = comp.c_next_window_id;
        let wm_tx = comp.c_wm_tx.clone();
        comp.c_surfaces.push(RefCell::new(Surface::new(
            id,
            res,
            wm_tx,
            0, 0
        )));
        // get a pointer to the refcell
        let entry_index = comp.c_surfaces.len() - 1;
        let surface_cell = &mut comp.c_surfaces[entry_index];

        // set the implementation for the wl_surface interface.
        // This means passing our new surface as the user data
        // field. The surface callbacks will need it.
        ws_resource_set_implementation(
            res, // the surfaces resource
            &SURFACE_INTERFACE,
            Some(surface_cell),
            Some(Surface::delete)
        );
    }

    // wl_compositor interface create region
    //
    // 
    pub extern "C" fn create_region(client: *mut wl_client,
                                    resource: *mut wl_resource,
                                    id: u32)
    {
        println!("Creating region");
    }

    // Present the surface for rendering
    //
    // This essentially makes the buffer available to the window manager
    // for drawing.
    pub fn render(&mut self) {
        for cell in self.c_surfaces.iter() {
            let surface = cell.borrow();

            if surface.s_committed_buffer.is_none() {
                continue;
            }
            
            // draw the window
        }
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
        let display = ws_display_create();
        // created at /var/run/user/1001/wayland-0
        let ret = ws_display_add_socket_auto(display);

        let event_loop = ws_display_get_event_loop(display);
        let loop_fd = ws_event_loop_get_fd(event_loop);

        let ret = ws_display_init_shm(display);

        let comp = RefCell::new(Compositor {
            c_clients: Vec::new(),
            c_surfaces: Vec::new(),
            c_wm_tx: wm_tx.clone(),
            c_next_window_id: 1,
        });

        let mut evman = Box::new(EventManager {
            em_compositor: comp,
            em_display: display,
            em_event_loop: event_loop,
            em_event_loop_fd: loop_fd,
            em_wm_tx: wm_tx,
            em_rx: rx,
        });

        // create interface for our compositor
        // this global is independent of any one client, and
        // will be the first thing they bind
        ws_global_create!(
            display,
            wl_compositor_interface,
            3,
            // add ourselves as the private data
            as_mut_c_void!(evman.em_compositor),
            Compositor::bind_compositor_callback
        );
        ws_global_create!(
            display,
            wl_shell_interface,
            1,
            std::ptr::null_mut(),
            wl_shell::bind_wl_shell
        );

        return evman;
    }
}

impl EventManager {
    pub fn worker_thread(&mut self) {
        loop {
            // wait for the next event
            self.event_loop_dispatch();
            self.flush_clients();

            //let task = self.rx.recv().unwrap();
            //self.process_task(&task);
        }
    }

    // Safe wrapper for wl_event_loop_dispatch
    //
    // dispatches requests to event handlers.
    // this is non-blocking.
    pub fn event_loop_dispatch(&mut self) {
        ws_event_loop_dispatch(self.em_event_loop, -1);
    }

    // Safe wrapper for wl_display_flush_clients
    //
    // Waits while events are sent to the clients through the
    // socket. Non-blocking, but will only send as much as
    // the socket can take at the moment.
    pub fn flush_clients(&mut self) {
        ws_display_flush_clients(self.em_display);
    }
}

// Destroy the compositor
//
// For now all we need to do is free the wl_display
impl Drop for EventManager {
    fn drop(&mut self) {
        ws_display_destroy(self.em_display);
    }
}
