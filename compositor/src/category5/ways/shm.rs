// Implementation of the wl_shm_interface
//
// Austin Shafer - 2020
//
// Inspired by the shm module in smithay
extern crate nix;
extern crate wayland_server as ws;

use ws::Main;
use ws::protocol::{wl_shm, wl_shm_pool};

use std::os::unix::io::RawFd;
use std::ffi::c_void;
use nix::sys::mman;

// A ShmRegion is a mmapped anonymous region of
// shared memory
//
// This region is mmapped from the fd passed
// by wl_shm
//
// It is the user_data for a shm pool
#[allow(dead_code)]
struct ShmRegion {
    sr_fd: RawFd,
    sr_raw_ptr: *mut c_void,
    sr_size: usize,
}

impl ShmRegion {
    fn new(fd: RawFd, size: usize) -> Option<ShmRegion> {
        unsafe {
            // To create the region we need to map size
            // bytes from fd
            let ptr = match mman::mmap(
                std::ptr::null_mut(),
                size,
                mman::ProtFlags::PROT_READ,
                mman::MapFlags::MAP_SHARED,
                fd,
                0)
            {
                Ok(p) => p,
                Err(_) => return None,
            };

            Some(ShmRegion {
                sr_fd: fd,
                sr_raw_ptr: ptr,
                sr_size: size,
            })
        }
    }
}

impl Drop for ShmRegion {
    fn drop(&mut self) {
        if !self.sr_raw_ptr.is_null() {
            unsafe {
                // We need to manually unmap this region whenever
                // it goes out of scope
                mman::munmap(self.sr_raw_ptr, self.sr_size).unwrap();
            }
        }
    }
}

pub fn shm_handle_request(req: wl_shm::Request,
                          shm: Main<wl_shm::WlShm>)
{
    match req {
        wl_shm::Request::CreatePool { id, fd, size } => {
            // We only handle valid sized pools
            if size <= 0 {
                shm.as_ref().post_error(
                    wl_shm::Error::InvalidFd as u32,
                    "Invalid Fd".to_string(),
                ); 
            }

            let reg = ShmRegion::new(fd, size as usize).unwrap();
            // Register a callback for the wl_shm_pool interface
            id.quick_assign(|p, r, _| {
                shm_pool_handle_request(r, p);
            });
            // Add our ShmRegion as the private data for the pool
            id.as_ref().user_data().set(move || reg);
        },
        _ => unimplemented!(),
    }
}

pub fn shm_pool_handle_request(req: wl_shm_pool::Request,
                               pool: Main<wl_shm_pool::WlShmPool>)
{
    let _reg = pool.as_ref().user_data().get::<ShmRegion>().unwrap();

    match req {
        #[allow(unused_variables)]
        wl_shm_pool::Request::CreateBuffer {
            id,
            offset,
            width,
            height,
            stride,
            format,
        } => {
            if format != wl_shm::Format::Xrgb8888 {
                pool.as_ref().post_error(
                    wl_shm::Error::InvalidFormat as u32,
                    format!("SHM format {:?} is not supported.", format),
                );
            }

            id.quick_assign(|_, _, _| {});
        },
        wl_shm_pool::Request::Destroy => {},
        _ => unimplemented!(),
    }
}
