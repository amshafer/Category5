// Safe bindings for common wayland functions
//
// Austin Shafer - 2020

use super::wayland_bindings::*;

// This needs to be a macro because
// interface will be a pointer to an extern
// struct, which is unsafe. We still want
// to attempt to provide a safe binding though
#[allow(unused_macros)]
#[macro_use]
macro_rules! ws_resource_create {
    ($client:ident,    // *mut wl_client,
     $interface:ident, // *const wl_interface,
     $version:expr,    // i32
     $id:expr) => {   // u32
        unsafe {
            wl_resource_create(
                $client, &$interface, $version, $id
            )
        }
    }
}

// This needs to be a macro because
// interface will be a pointer to an extern
// struct, which is unsafe. We still want
// to attempt to provide a safe binding though
#[allow(unused_macros)]
#[macro_use]
macro_rules! ws_global_create {
    ($display:ident,    // *mut wl_display,
     $interface:ident,  // *const wl_interface,
     $version:expr,     // i32
     $data:ident,       // Box
     $bind:expr) => {   // wl_global_bind_func_t
        unsafe {
            wl_global_create(
                $display,
                &$interface,
                $version,
                &mut *$data as *mut _ as *mut std::ffi::c_void,
                Some($bind)
            )
        }
    }
}

pub fn ws_resource_set_implementation<T, D>
    (resource: *mut wl_resource,
    implementation: &T,
    data: &mut D,
    destroy: wl_resource_destroy_func_t)
{
    unsafe {
        wl_resource_set_implementation(
            resource,
            implementation
                as *const _ as *const std::ffi::c_void,
            // this will be the Compositor *mut self
            data as *mut _ as *mut std::ffi::c_void,
            None
        );
    }
}

pub fn ws_event_loop_dispatch(
    loop_: *mut wl_event_loop,
    timeout: ::std::os::raw::c_int)
    -> ::std::os::raw::c_int
{
    unsafe {
        wl_event_loop_dispatch(loop_, timeout)
    }
}

pub fn ws_display_flush_clients(display: *mut wl_display) {
    unsafe {
        wl_display_flush_clients(display);
    }
}

pub fn ws_display_create() -> *mut wl_display {
    unsafe { wl_display_create() }
}

pub fn ws_display_destroy(display: *mut wl_display) {
    unsafe { wl_display_destroy(display) }
}

pub fn ws_display_get_event_loop(display: *mut wl_display)
                                 -> *mut wl_event_loop
{
    unsafe { wl_display_get_event_loop(display) }
}

pub fn ws_display_add_socket_auto(display: *mut wl_display) {
    unsafe { wl_display_add_socket_auto(display); }
}

pub fn ws_event_loop_get_fd(loop_: *mut wl_event_loop)
                            -> ::std::os::raw::c_int
{
    unsafe { wl_event_loop_get_fd(loop_) }
}

pub fn ws_display_init_shm(display: *mut wl_display)
                           -> ::std::os::raw::c_int
{
    unsafe { wl_display_init_shm(display) }
}
