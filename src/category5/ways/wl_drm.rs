// Implementation of mesa's wl_drm
// interfaces for importing GPU buffers into
// vkcomp.
//
// https://wayland.app/protocols/wayland-drm#wl_drm
//
// Austin Shafer - 2021
extern crate wayland_server as ws;

use crate::category5::atmosphere::Atmosphere;
use std::cell::RefCell;
use std::rc::Rc;
use utils::log;
use ws::Main;

use nix::sys::stat::SFlag;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int};

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
extern "C" {
    pub fn devname_r(
        #[cfg(target_arch = "x86_64")] dev: u64, // 64 bit on amd64
        mode: libc::mode_t,
        buf: *mut libc::c_char,
        len: libc::c_int,
    ) -> *mut libc::c_char;
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
fn makedev(major: u64, minor: u64) -> u64 {
    (((major & 0xffffff00) as u64) << 32)
        | (((major & 0xff) as u64) << 8)
        | ((minor & 0xff00 as u64) << 24)
        | (minor & 0xffff00ff)
}

fn get_drm_dev_name(atmos: &Atmosphere) -> String {
    let (major, minor) = atmos.get_drm_dev();

    let mut dev_name = Vec::<c_char>::with_capacity(256); // Matching value of SPECNAMELEN in FreeBSD 13+

    let buf: *mut c_char = unsafe {
        devname_r(
            makedev(major as u64, minor as u64),
            SFlag::S_IFCHR.bits(), // Must be S_IFCHR or S_IFBLK
            dev_name.as_mut_ptr(),
            dev_name.capacity() as c_int,
        )
    };

    assert!(!buf.is_null());

    // SAFETY: The buffer written to by devname_r is guaranteed to be NUL terminated.
    let cstr = unsafe { CStr::from_ptr(buf) };
    // This will be of form /dev/drm/128
    let full_drm_name = format!("/dev/{}", cstr.to_string_lossy().into_owned());
    assert!(full_drm_name.starts_with("/dev/drm/"));

    // Turn this into /dev/dri/renderD128
    // 9 characters in /dev/drm/
    let drm_number_in_name = &full_drm_name[9..];
    format!("/dev/dri/renderD{}", drm_number_in_name)
}

pub fn wl_drm_setup(atmos_rc: Rc<RefCell<Atmosphere>>, wl_drm: Main<wl_drm::WlDrm>) {
    println!("LIBC DEV_T = {:?}", std::any::type_name::<libc::dev_t>());
    // Send the name of the DRM device reported by vkcomp
    let atmos = atmos_rc.borrow();
    let drm_name = get_drm_dev_name(&atmos);
    log::error!("DRM device returned by wl_drm is {}", drm_name);

    wl_drm.device(drm_name);
}

/// Ignores all requests. We only use this protocol to deliver
/// the drm name.
pub fn wl_drm_handle_request(req: wl_drm::Request, _wl_drm: Main<wl_drm::WlDrm>) {
    log::error!("Unimplemented wl_drm request {:?}", req);
}
