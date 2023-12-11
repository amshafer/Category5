// Implementation of mesa's wl_drm
// interfaces for importing GPU buffers into
// vkcomp.
//
// https://wayland.app/protocols/wayland-drm#wl_drm
//
// Austin Shafer - 2021
extern crate wayland_server as ws;

use crate::category5::atmosphere::Atmosphere;
use crate::category5::Climate;
use utils::log;

use nix::sys::stat::SFlag;
use std::ffi::CStr;
use std::ops::DerefMut;
use std::os::raw::c_char;

#[cfg(target_os = "linux")]
use nix::sys::stat::makedev;

use super::protocol::wl_drm::wl_drm;

// Alright now this gets gross
//
// Basically there's a big fight in libc about compatability between
// different versions, i.e. the devparameter of devname_r may be
// 32-bit on one freebsd version and 64 bit later. They still don't
// have a way to fix this, and choose the lowest common denominator,
// causing dev_t to be 32-bit in the libc crate, which is big wrong.
//
// https://github.com/rust-lang/libc/pull/2603#issuecomment-1003715259
#[cfg(target_os = "freebsd")]
extern "C" {
    pub fn devname(
        #[cfg(target_arch = "x86_64")] dev: u64, // 64 bit on amd64
        mode: libc::mode_t,
    ) -> *const libc::c_char;
}

/// In FreeBSD types.h:
///
/// ```
/// #define makedev(M, m)   __makedev((M), (m))
/// static __inline dev_t
/// __makedev(int _Major, int _Minor)
/// {
///     return (((dev_t)(_Major & 0xffffff00) << 32) | ((_Major & 0xff) << 8) |
///         ((dev_t)(_Minor & 0xff00) << 24) | (_Minor & 0xffff00ff));
/// }
/// ```
#[cfg(target_os = "freebsd")]
fn makedev(major: u64, minor: u64) -> u64 {
    (((major & 0xffffff00) as u64) << 32)
        | (((major & 0xff) as u64) << 8)
        | ((minor & 0xff00 as u64) << 24)
        | (minor & 0xffff00ff)
}

#[cfg(target_os = "freebsd")]
fn get_drm_dev_name(atmos: &Atmosphere) -> String {
    let (major, minor) = atmos.get_drm_dev();

    let raw_name: *const c_char = unsafe {
        devname(
            makedev(major as u64, minor as u64),
            SFlag::S_IFCHR.bits(), // Must be S_IFCHR or S_IFBLK
        )
    };

    assert!(!raw_name.is_null());

    // SAFETY: The buffer written to by devname_r is guaranteed to be NUL terminated.
    let cstr = unsafe { CStr::from_ptr(raw_name) };
    // This will be of form /dev/drm/128
    let full_drm_name = format!("/dev/{}", cstr.to_string_lossy().into_owned());
    assert!(full_drm_name.starts_with("/dev/drm/"));

    // Turn this into /dev/dri/renderD128
    // 9 characters in /dev/drm/
    let drm_number_in_name = &full_drm_name[9..];
    format!("/dev/dri/renderD{}", drm_number_in_name)
}

/// TODO: Linux stupidly doesn't have a good way to get a device path given
/// a dev_t, so we are just hard-coding the first available device here
#[cfg(target_os = "linux")]
fn get_drm_dev_name(_atmos: &Atmosphere) -> String {
    return "/dev/dri/renderD128".to_string();
}

#[allow(unused_variables)]
impl ws::GlobalDispatch<wl_drm::WlDrm, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<wl_drm::WlDrm>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        let wl_drm = data_init.init(resource, ());
        println!("LIBC DEV_T = {:?}", std::any::type_name::<libc::dev_t>());
        // Send the name of the DRM device reported by vkcomp
        let drm_name = get_drm_dev_name(state.c_atmos.lock().unwrap().deref_mut());
        log::error!("DRM device returned by wl_drm is {}", drm_name);

        wl_drm.device(drm_name);
        wl_drm.capabilities(wl_drm::Capability::Prime.into())
    }
}

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wl_drm::WlDrm, ()> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_drm::WlDrm,
        request: wl_drm::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        log::error!("Unimplemented wl_drm request {:?}", request);
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: &wl_drm::WlDrm,
        data: &(),
    ) {
    }
}
