// Safe bindings for common wayland functions
//
// Austin Shafer - 2020

use super::wayland_bindings::*;
use crate::category5::utils::MemImage;

// This needs to be a macro because
// interface will be a pointer to an extern
// struct, which is unsafe. We still want
// to attempt to provide a safe binding though
#[allow(unused_macros)]
#[macro_use]
macro_rules! ws_resource_create {
    ($client:expr,    // WLClient
     $interface:ident, // wl_interface
     $version:expr,    // i32
     $id:expr) => {    // u32
        unsafe {
            assert!(!$client.ptr.is_null());
            let ret = wl_resource_create(
                $client.ptr, &$interface, $version, $id
            );
            assert!(!ret.is_null());
            WLResource { ptr: ret }
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
    ($display:ident,    // WLDisplay
     $interface:ident,  // wl_interface,
     $version:expr,     // i32
     $data:expr,        // *mut c_void
     $bind:expr) => {   // wl_global_bind_func_t
        unsafe {
            assert!(!$display.ptr.is_null());
            let ret = wl_global_create(
                $display.ptr,
                &$interface,
                $version,
                $data,
                Some($bind)
            );
            assert!(!ret.is_null());
            WLGlobal { ptr: ret }
        }
    }
}

macro_rules! wl_type_wrapper {
    ($wltype:ident, $tname:ident) => {
        #[derive(Copy, Clone)]
        pub struct $tname {
            pub ptr: *mut $wltype,
        }
        impl $tname {
            pub fn from_ptr(ptr: *mut $wltype) -> $tname {
                $tname {
                    ptr: ptr,
                }
            }
        }
    }
}

wl_type_wrapper!(wl_client, WLClient);
wl_type_wrapper!(wl_display, WLDisplay);
wl_type_wrapper!(wl_resource, WLResource);
wl_type_wrapper!(wl_global, WLGlobal);
wl_type_wrapper!(wl_shm_buffer, WLShmBuffer);

#[derive(Copy, Clone)]
pub struct WLEventLoop {
    pub fd: i32,
    pub ptr: *mut wl_event_loop
}
#[derive(Copy, Clone)]
pub struct WLShm {
    pub fd: i32,
}

// Set the implementation of the resource
//
// If using None for data, then you will need to do:
// https://github.com/rust-lang/rust/issues/39797
//
// i.e. None::<&mut Compositor> or something
pub fn ws_resource_set_implementation<T, D>
    (resource: WLResource,
    implementation: &T,
    data: Option<&mut D>,
    destroy: wl_resource_destroy_func_t)
{
    assert!(!resource.ptr.is_null());
    unsafe {
        wl_resource_set_implementation(
            resource.ptr,
            implementation
                as *const _ as *const std::ffi::c_void,
            // this will be the Compositor *mut self
            match data {
                Some(d) => d as *mut _ as *mut std::ffi::c_void,
                None => std::ptr::null_mut(),
            },
            destroy,
        );
    }
}

pub fn ws_event_loop_dispatch(
    evloop: WLEventLoop,
    timeout: ::std::os::raw::c_int)
    -> ::std::os::raw::c_int
{
    assert!(!evloop.ptr.is_null());
    unsafe {
        let ret = wl_event_loop_dispatch(evloop.ptr, timeout);
        assert!(ret >= 0);
        return ret;
    }
}

pub fn ws_display_flush_clients(display: WLDisplay) {
    assert!(!display.ptr.is_null());
    unsafe {
        wl_display_flush_clients(display.ptr);
    }
}

pub fn ws_display_create() -> WLDisplay {
    unsafe { WLDisplay{ ptr: wl_display_create() } }
}

pub fn ws_display_destroy(display: WLDisplay) {
    assert!(!display.ptr.is_null());
    unsafe { wl_display_destroy(display.ptr) }
}

pub fn ws_event_loop_get_fd(evloop: WLEventLoop)
                            -> ::std::os::raw::c_int
{
    assert!(!evloop.ptr.is_null());
    assert!(evloop.fd >= 0);
    return evloop.fd;
}

pub fn ws_display_get_event_loop(display: WLDisplay)
                                 -> WLEventLoop
{
    assert!(!display.ptr.is_null());
    unsafe {
        let ptr = wl_display_get_event_loop(display.ptr);
        let fd = wl_event_loop_get_fd(ptr);

        WLEventLoop {
            fd: fd,
            ptr: ptr,
        }
    }
}

pub fn ws_display_add_socket_auto(display: WLDisplay)
{
    assert!(!display.ptr.is_null());
    unsafe { wl_display_add_socket_auto(display.ptr); }
}

pub fn ws_display_init_shm(display: WLDisplay)
                           -> WLShm
{
    assert!(!display.ptr.is_null());
    unsafe {
        let fd = wl_display_init_shm(display.ptr);
        assert!(fd >= 0);
        WLShm {
            fd: fd,
        }
    }
}

pub fn ws_shm_buffer_get(resource: WLResource) -> WLShmBuffer {
    assert!(!resource.ptr.is_null());
    unsafe {
        let ptr = wl_shm_buffer_get(resource.ptr);
        assert!(!ptr.is_null());

        WLShmBuffer {
            ptr: ptr,
        }
    }
}

pub fn ws_shm_buffer_begin_access(buffer: WLShmBuffer) {
    assert!(!buffer.ptr.is_null());
    unsafe {
        wl_shm_buffer_begin_access(buffer.ptr);
    }
}

pub fn ws_shm_buffer_end_access(buffer: WLShmBuffer) {
    assert!(!buffer.ptr.is_null());
    unsafe {
        wl_shm_buffer_end_access(buffer.ptr);
    }
}

pub fn ws_shm_buffer_get_data(buffer: WLShmBuffer) -> MemImage {
    assert!(!buffer.ptr.is_null());
    let width = ws_shm_buffer_get_width(buffer);
    let height = ws_shm_buffer_get_width(buffer);

    unsafe {
        let raw = wl_shm_buffer_get_data(buffer.ptr)
            as *mut u8;
        assert!(!raw.is_null());

        // Size of each pixel is 4 bytes
        MemImage::new(raw, 4, width as usize, height as usize)
    }
}

pub fn ws_shm_buffer_get_stride(buffer: WLShmBuffer) -> i32 {
    assert!(!buffer.ptr.is_null());
    unsafe {
        wl_shm_buffer_get_stride(buffer.ptr)
    }
}

pub fn ws_shm_buffer_get_format(buffer: WLShmBuffer) -> u32 {
    assert!(!buffer.ptr.is_null());
    unsafe {
        wl_shm_buffer_get_format(buffer.ptr)
    }
}

pub fn ws_shm_buffer_get_width(buffer: WLShmBuffer) -> i32 {
    assert!(!buffer.ptr.is_null());
    unsafe {
        wl_shm_buffer_get_width(buffer.ptr)
    }
}

pub fn ws_shm_buffer_get_height(buffer: WLShmBuffer) -> i32 {
    assert!(!buffer.ptr.is_null());
    unsafe {
        wl_shm_buffer_get_height(buffer.ptr)
    }
}
