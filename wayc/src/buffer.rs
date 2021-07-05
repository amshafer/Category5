use crate::wc::protocol::{wl_buffer::*, wl_shm::*};
use crate::wc::Main;
use crate::{Context, Result, Wayc};
use std::cell::RefCell;
use std::os::unix::io::RawFd;
use std::rc::Rc;

use libc::SHM_ANON;
use nix::sys::mman;
use std::ffi::CStr;

/// Defines what type of handle this buffer is.
pub enum BufferType {
    /// This buffer is backed by shared memory
    Shm { shm_fd: RawFd }, // todo, add wl_shm buffer handle
    /// This buffer is backed by a dmabuf handle
    DmaBuf,
}

pub type BufferHandle = Rc<RefCell<Buffer>>;

/// A buffer is a set of memory used to define the contents of a surface
pub struct Buffer {
    pub(crate) b_size: (usize, usize),
    pub(crate) b_type: BufferType,
    pub(crate) b_wl_buf: Main<WlBuffer>,
}

impl Buffer {
    pub fn new_shm(wayc: &mut Wayc, width: usize, height: usize) -> Result<BufferHandle> {
        let shm = mman::shm_open(
            unsafe { CStr::from_ptr(libc::SHM_ANON) },
            nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .context("Could not shm_open shared memory for wl_buffer")?;

        // now extend our shm to the buffer dimensions
        let fsize = width * height * 4; // TODO: make Format ??
        nix::unistd::ftruncate(shm, fsize as i64)
            .context("Could not ftruncate shared memory for wl_buffer")?;

        // now we can tell the compositor to make a wl_buffer for us, backed
        // by our shared memory
        let shm_pool = wayc.c_shm.create_pool(shm, fsize as i32);
        let shm_buf = shm_pool.create_buffer(
            0,
            width as i32,
            height as i32,
            (width * height) as i32, // stride
            Format::Argb8888,
        );

        Ok(Rc::new(RefCell::new(Self {
            b_size: (width, height),
            b_type: BufferType::Shm { shm_fd: shm },
            b_wl_buf: shm_buf,
        })))
    }
}
