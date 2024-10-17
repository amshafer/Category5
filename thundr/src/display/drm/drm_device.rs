/// DRM Device
///
/// Austin - 2024
#[cfg(target_os = "linux")]
use nix::sys::stat::makedev;

use crate::display::drm::drm::Device;

use std::sync::{Arc, Mutex};
use utils::log;

// In FreeBSD types.h:
//
// #define makedev(M, m)   __makedev((M), (m))
// static __inline dev_t
// __makedev(int _Major, int _Minor)
// {
//     return (((dev_t)(_Major & 0xffffff00) << 32) | ((_Major & 0xff) << 8) |
//         ((dev_t)(_Minor & 0xff00) << 24) | (_Minor & 0xffff00ff));
// }
#[cfg(target_os = "freebsd")]
fn makedev(major: u64, minor: u64) -> u64 {
    (((major & 0xffffff00) as u64) << 32)
        | (((major & 0xff) as u64) << 8)
        | ((minor & 0xff00 as u64) << 24)
        | (minor & 0xffff00ff)
}

/// Our DRM node accessor helper
///
/// This provides drm-rs with access to the DRM fd
/// and gives us a place to make calls to DRM
pub struct DrmDevice {
    ds_drm_fd: std::fs::File,
}

/// Implementing `AsFd` is a prerequisite to implementing the traits found
/// in this crate. Here, we are just calling `as_fd()` on the inner File.
impl std::os::unix::io::AsFd for DrmDevice {
    fn as_fd(&self) -> std::os::unix::io::BorrowedFd<'_> {
        self.ds_drm_fd.as_fd()
    }
}

impl Device for DrmDevice {}
impl drm::control::Device for DrmDevice {}

impl DrmDevice {
    pub fn new(major: i64, minor: i64) -> Option<Arc<Mutex<Self>>> {
        let dev_t = makedev(major as u64, minor as u64);
        #[cfg(target_os = "freebsd")]
        let dev_t = dev_t as u32;
        let path = match drm::node::dev_path(dev_t.into(), drm::node::NodeType::Primary) {
            Ok(path) => path,
            Err(e) => {
                log::error!("Could not get DRM path from dev_t {}: {}", dev_t, e);
                return None;
            }
        };

        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        options.write(true);
        let file = match options.open(&path) {
            Ok(file) => file,
            Err(e) => {
                log::error!("Could not open DRM Device path {}: {}", path.display(), e);
                return None;
            }
        };

        let ret = DrmDevice { ds_drm_fd: file };
        // Request any properties needed
        if ret
            .set_client_capability(drm::ClientCapability::UniversalPlanes, true)
            .is_err()
        {
            log::error!("Failed to request UniversalPlanes capability");
            return None;
        }
        if ret
            .set_client_capability(drm::ClientCapability::Atomic, true)
            .is_err()
        {
            log::error!("Failed to request Atomic capability");
            return None;
        }

        return Some(Arc::new(Mutex::new(ret)));
    }
}
