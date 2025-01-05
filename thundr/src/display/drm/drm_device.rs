/// DRM Device
///
/// Austin - 2024
extern crate gbm;
#[cfg(target_os = "linux")]
use nix::sys::stat::makedev;

use crate::display::drm::drm::Device;
use crate::utils::{Context, Result};

use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};

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
    /// Our gbm_device.
    pub ds_gbm: gbm::Device<std::os::fd::OwnedFd>,
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
    pub fn new(major: i64, minor: i64) -> Result<Arc<Mutex<Self>>> {
        let dev_t = makedev(major as u64, minor as u64);
        #[cfg(target_os = "freebsd")]
        let dev_t = dev_t as u32;
        let path = drm::node::dev_path(dev_t.into(), drm::node::NodeType::Primary)
            .context(format!("Could not get DRM path from dev_t {}", dev_t))?;

        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        options.write(true);
        let file = options
            .open(&path)
            .context(format!("Could not open DRM Device path {}", path.display()))?;

        let gbm = gbm::Device::new(file.as_fd().try_clone_to_owned()?)
            .context("Could not create GBM Device")?;

        let ret = DrmDevice {
            ds_drm_fd: file,
            ds_gbm: gbm,
        };

        // Request any properties needed
        ret.set_client_capability(drm::ClientCapability::UniversalPlanes, true)
            .context("Failed to request UniversalPlanes capability")?;

        ret.set_client_capability(drm::ClientCapability::Atomic, true)
            .context("Failed to request Atomic capability")?;

        return Ok(Arc::new(Mutex::new(ret)));
    }
}
