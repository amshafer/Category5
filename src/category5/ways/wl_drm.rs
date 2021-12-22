// Implementation of mesa's wl_drm
// interfaces for importing GPU buffers into
// vkcomp.
//
// https://wayland.app/protocols/wayland-drm#wl_drm
//
// Austin Shafer - 2021
extern crate wayland_server as ws;

use utils::log;
use ws::Main;

use super::protocol::wl_drm::wl_drm;

pub fn wl_drm_setup(wl_drm: Main<wl_drm::WlDrm>) {
    // Send the name of the DRM device reported by vkcomp
}

/// Ignores all requests. We only use this protocol to deliver
/// the drm name.
pub fn wl_drm_handle_request(req: wl_drm::Request, _wl_drm: Main<wl_drm::WlDrm>) {
    log::error!("Unimplemented wl_drm request {:?}", req);
}
