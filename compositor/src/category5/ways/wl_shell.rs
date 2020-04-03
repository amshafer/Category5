// Interface for wl_shell_interface
//
// Austin Shafe - 2020

use super::wayland_bindings::*;
use super::wayland_safe::*;
use super::compositor::Compositor;

use std::cell::RefCell;

pub extern "C" fn bind_wl_shell(client: *mut wl_client,
                                data: *mut ::std::os::raw::c_void,
                                version: u32,
                                id: u32)
{
    let comp_ref = unsafe {
        // Get a slice of one Compositor, then grab a ref
        // to the first one
        &mut std::slice::from_raw_parts_mut(
            data as *mut RefCell<Compositor>, 1)[0]
    };

    let res = ws_resource_create!(
        WLClient::from_ptr(client),
        wl_shell_interface,
        1, id
    );
    ws_resource_set_implementation(
        res,
        &WL_SHELL_INTERFACE,
        Some(comp_ref),
        None
    );
}

pub static WL_SHELL_INTERFACE: wl_shell_interface =
    wl_shell_interface {
    get_shell_surface: Some(get_shell_surface),
};

extern "C" fn get_shell_surface(
    client: *mut wl_client,
    resource: *mut wl_resource,
    id: u32,
    surface: *mut wl_resource)
{
    let comp_ref = get_userdata_raw!(resource, Compositor)
        .unwrap();

    let res = ws_resource_create!(
        WLClient::from_ptr(client),
        wl_shell_interface,
        1, id
    );
    ws_resource_set_implementation(
        res,
        &WL_SHELL_INTERFACE,
        Some(comp_ref),
        None
    );
}

pub static WL_SHELL_SURFACE_INTERFACE: wl_shell_surface_interface =
    wl_shell_surface_interface {
    pong: Some(pong),
    move_: Some(move_),
    resize: Some(resize),
    set_toplevel: Some(set_toplevel),
    set_transient: Some(set_transient),
    set_fullscreen: Some(set_fullscreen),
    set_popup: Some(set_popup),
    set_maximized: Some(set_maximized),
    set_title: Some(set_title),
    set_class: Some(set_class),
};

extern "C" fn pong(client: *mut wl_client,
                   resource: *mut wl_resource,
                   serial: u32)
{

}

extern "C" fn move_(
    client: *mut wl_client,
    resource: *mut wl_resource,
    seat: *mut wl_resource,
    serial: u32)
{

}
extern "C" fn resize(
    client: *mut wl_client,
    resource: *mut wl_resource,
    seat: *mut wl_resource,
    serial: u32,
    edges: u32)
{

}

extern "C" fn set_toplevel(client: *mut wl_client,
                           resource: *mut wl_resource)
{

}

extern "C" fn set_transient(
    client: *mut wl_client,
    resource: *mut wl_resource,
    parent: *mut wl_resource,
    x: i32,
    y: i32,
    flags: u32)
{

}

extern "C" fn set_fullscreen(
    client: *mut wl_client,
    resource: *mut wl_resource,
    method: u32,
    framerate: u32,
    output: *mut wl_resource)
{

}

extern "C" fn set_popup(
    client: *mut wl_client,
    resource: *mut wl_resource,
    seat: *mut wl_resource,
    serial: u32,
    parent: *mut wl_resource,
    x: i32,
    y: i32,
    flags: u32)
{

}

extern "C" fn set_maximized(
    client: *mut wl_client,
    resource: *mut wl_resource,
    output: *mut wl_resource)
{

}

extern "C" fn set_title(
    client: *mut wl_client,
    resource: *mut wl_resource,
    title: *const ::std::os::raw::c_char)
{

}

extern "C" fn set_class(
    client: *mut wl_client,
    resource: *mut wl_resource,
    class_: *const ::std::os::raw::c_char)
{

}
