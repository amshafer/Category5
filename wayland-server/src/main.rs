// Wayland binding fun fun fun
//
// put this in the bindings
//   #![allow(non_snake_case, non_camel_case_types, non_upper_case_globals)]
//
// Austin Shafer - 2019
extern crate minifb;

mod wayland_bindings;
use wayland_bindings::*;

use minifb::{WindowOptions, Window};
use std::ptr;
use std::cell::RefCell;

// wl_surface_interface implementation
//
// This is used for assigning buffers to a visible area on screen. A
// surface just represents a visible area. The attach and commit hooks
// are the most interesting. More complete info can be found in the auto
// generated docs of wayland_bindings.
static surface_interface: wl_surface_interface = wl_surface_interface {
    destroy: None,
    // Attaches a wl_buffer to the surface which represents the window contents
    attach: Some(Surface::attach),
    damage: None,
    frame: None,
    set_opaque_region: None,
    set_input_region: None,
    // Gives the compositor "ownership" of the current buffer, for presentation
    commit: Some(Surface::commit),
    set_buffer_transform: None,
    set_buffer_scale: None,
    damage_buffer: None,
};

// Private structure for a wayland surface
//
// A surface represents a visible area on screen. Desktop organization
// effects and other transformations are taken care of by a 'shell'
// interface, not this. A surface will have a buffer attached to it which
// will be displayed to the client when it is committed.
pub struct Surface {
    // A resource representing a wl_surface. (the 'real' surface)
    s_surface: *mut wl_resource,
    // The currently attached buffer. Will be displayed on commit
    // When the window is created a buffer is not assigned, hence the option
    s_attached_buffer: Option<*mut wl_resource>,
    // the s_attached_buffer is moved here to signify that we can draw
    // with it.
    s_committed_buffer: Option<*mut wl_resource>,
    // the location of the surface in our compositor
    s_x: u32,
    s_y: u32,
}

impl Surface {

    // attach a wl_buffer to the surface
    //
    // The client crafts a buffer with care, and tells us that it will be
    // backing the surface represented by `resource`. `buffer` will be
    // placed in the private struct that the compositor made.
    pub extern "C" fn attach(client: *mut wl_client,
                             resource: *mut wl_resource,
                             buffer: *mut wl_resource,
                             x: i32,
                             y: i32)
    {
        // get our private struct and assign it the buffer
        // that the client has attached
        let cell: *mut RefCell<Surface> = unsafe {
            wl_resource_get_user_data(resource) as *mut RefCell<Surface>
        };
        if cell == ptr::null_mut() {
            return;
        }

        let mut surface = unsafe { (*cell).borrow_mut() };

        surface.s_attached_buffer = Some(buffer);
    }

    pub extern "C" fn commit(client: *mut wl_client,
                             resource: *mut wl_resource)
    {
        // only do shm for now
        let cell: *mut RefCell<Surface> = unsafe {
            wl_resource_get_user_data(resource) as *mut RefCell<Surface>
        };
        if cell == ptr::null_mut() {
            return;
        }

        let mut surface = unsafe { (*cell).borrow_mut() };

        // the wl_shm_buffer object, not the framebuffer
        if !surface.s_attached_buffer.is_none() {
            surface.s_committed_buffer = surface.s_attached_buffer;
        }
    }

    pub extern "C" fn delete(resource: *mut wl_resource) {
        let surface: *mut Surface = unsafe {
            wl_resource_get_user_data(resource) as *mut Surface
        };
        if surface == ptr::null_mut() {
            return;
        }
    }

    // create a new visible surface at coordinates (x,y)
    // from the specified wayland resource
    pub fn new(res: *mut wl_resource, x: u32, y: u32) -> Surface {
        Surface {
            s_surface: res,
            s_attached_buffer: None,
            s_committed_buffer: None,
            s_x: x,
            s_y: y,
        }
    }
}

// A wayland compositor wrapper
//
// This class is point of first contact from higher levels in the stack.
// It roughly holds the wl_compositor/wl_display/wl_event_loop objects.
// There should be as little compositor logic in this as possible, most
// of the design decisions and actions should be taken by higher levels, using
// this API to hide the unsafe bits.
pub struct Compositor {
    // The wayland display object, this is the core
    // global singleton for libwayland
    c_display: *mut wl_display,
    // This struct holds the event loop, as we want to abstract
    // the unsafe code for flushing clients and waiting for events
    c_event_loop: *mut wl_event_loop,
    c_event_loop_fd: i32,
    // A list of wayland client representations. These are the
    // currently connected clients.
    c_clients: Vec<RefCell<u32>>,
    // A list of surfaces which have been handed out to clients
    c_surfaces: Vec<RefCell<Surface>>,
    // the mini framebuffer we are using for now
    c_window: Window,
    c_fb: [u32; 1024 * 1024],
}

// wlci - wl compositor interface
//
// our implementation of wl_compositor_interface. The compositor singleton
// kicks off the window creation process by creating a surface
static compositor_interface: wl_compositor_interface = wl_compositor_interface {
    // called asynchronously by the client when they want to request a surface
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
    pub extern "C" fn bind_compositor_callback(client: *mut wl_client,
                                               data: *mut ::std::os::raw::c_void,
                                               version: u32,
                                               id: u32)
    {
        println!("Binding the compositor interface");

        unsafe {
            let res = wl_resource_create(client, &wl_compositor_interface, 1, id);
            wl_resource_set_implementation(
                res,
                &compositor_interface as *const _ as *const std::ffi::c_void,
                data, // this will be the Compositor *mut self
                None
            );
        }
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
        let comp: *mut Compositor = unsafe {
            wl_resource_get_user_data(resource) as *mut Compositor
        };
        if comp == ptr::null_mut() {
            return;
        }

        // first get a new resource to represent our surface
        let res = unsafe {
            wl_resource_create(client, &wl_surface_interface, 3, id)
        };
        
        unsafe {
            // create an entry in the surfaces list
            (*comp).c_surfaces.push(RefCell::new(Surface::new(res, 0, 0)));
            // get a pointer to the refcell
            let surface_cell = &mut (*comp).c_surfaces[(*comp).c_surfaces.len() - 1];

            // set the implementation for the wl_surface interface. This means
            // passing our new surface as the user data field. The surface callbacks
            // will need it.
            wl_resource_set_implementation(
                res, // the surfaces resource
                &surface_interface as *const _ as *const std::ffi::c_void,
                surface_cell as *mut _ as *mut std::ffi::c_void,
                Some(Surface::delete));
        }
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

    // Safe wrapper for wl_event_loop_dispatch
    //
    // dispatches requests to event handlers.
    // this is non-blocking.
    pub fn event_loop_dispatch(&mut self) {
        unsafe {
            wl_event_loop_dispatch(self.c_event_loop, -1);
        }
    }

    // Safe wrapper for wl_display_flush_clients
    //
    // Waits while events are sent to the clients through the
    // socket. Non-blocking, but will only send as much as
    // the socket can take at the moment.
    pub fn flush_clients(&mut self) {
        unsafe {
            wl_display_flush_clients(self.c_display);
        }
    }

    pub fn render(&mut self) {
        for cell in self.c_surfaces.iter() {
            let surface = cell.borrow();

            if surface.s_committed_buffer.is_none() {
                continue;
            }
            
            unsafe {
                let shm_buff = wl_shm_buffer_get(surface.s_committed_buffer.unwrap());
                let width = wl_shm_buffer_get_width(shm_buff);
                let height = wl_shm_buffer_get_height(shm_buff);
                // this is the raw data
                let fb_raw = wl_shm_buffer_get_data(shm_buff) as *mut _ as *mut u32;
                let fb: &[u32] =
                    core::slice::from_raw_parts(fb_raw,
                                                (4 * width * height) as usize);

                for i in 0..height {
                    for j in 0..width {
                        let off: usize = (surface.s_x + surface.s_y * 1024) as usize;
                        let idx: usize = (i * width + j) as usize;
                        self.c_fb[off + (i * 1024 + j) as usize] = fb[idx];
                    }
                }
            }

            self.c_window.update_with_buffer_size(&self.c_fb, 1024, 1024).unwrap();
        }
    }

    // Returns a new Compositor struct
    //
    // This creates a new wayland compositor, setting up all the needed resources
    // for the struct. It will create a wl_display, initialize a new socket,
    // create the client/surface lists, and create a compositor global resource.
    // This kicks off the global callback chain, starting with
    //    Compositor::bind_compositor_callback
    pub fn new() -> Box<Compositor> {
        unsafe {
            let display = wl_display_create();
            // created at /var/run/user/1001/wayland-0
            let ret = wl_display_add_socket_auto(display);
            
            let event_loop = wl_display_get_event_loop(display);
            let loop_fd = wl_event_loop_get_fd(event_loop);

            let ret = wl_display_init_shm(display);

            let mut comp = Box::new(Compositor {
                c_display: display,
                c_event_loop: event_loop,
                c_event_loop_fd: loop_fd,
                c_clients: Vec::new(),
                c_surfaces: Vec::new(),
                c_window: Window::new("Wayland Compositor",
                                     1024, // width
                                     1024, // height
                                     WindowOptions::default()).unwrap_or_else(|e| {
                                         panic!("{}", e);
                                     }),
                c_fb: [0; 1024 * 1024],
            });

            // create interface for our compositor
            // this global is independent of any one client, and will be the
            // first thing they bind
            let global = wl_global_create(
                display,
                &wl_compositor_interface,
                3,
                // add ourselves as the private data
                &mut *comp as *mut _ as *mut std::ffi::c_void,
                Some(Compositor::bind_compositor_callback)
            );

            return comp;
        }
    }
}

// Destroy the compositor
//
// For now all we need to do is free the wl_display
impl Drop for Compositor {
    fn drop(&mut self) {
        unsafe {
            wl_display_destroy(self.c_display);            
        }
    }
}

fn main() {
    let mut comp = Compositor::new();
    
    loop {
        // wait for the next event
        comp.event_loop_dispatch();

        comp.flush_clients();

        comp.render();
    }
}
