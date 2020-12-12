// Implementation of the wl_shm_interface
//
// Austin Shafer - 2020
//
// Inspired by the shm module in smithay
extern crate nix;
extern crate wayland_server as ws;

use ws::protocol::{wl_shm, wl_shm_pool};

use utils::{log, MemImage};

use nix::{sys::mman, unistd};
use std::cell::RefCell;
use std::ffi::c_void;
use std::ops::Deref;
use std::os::unix::io::RawFd;
use std::rc::Rc;

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
    // Create a new shared memory region from fd
    //
    // Maps size bytes of the fd as a shared memory region
    // in which the clients can reference data
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
                0,
            ) {
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

    // Enlarge the shm pool
    // Shrinking a pool is not supported
    fn resize(&mut self, size: usize) {
        assert!(self.sr_size <= size);
        self.sr_size = size;

        self.sr_raw_ptr = unsafe {
            match mman::mmap(
                std::ptr::null_mut(),
                self.sr_size,
                mman::ProtFlags::PROT_READ,
                mman::MapFlags::MAP_SHARED,
                self.sr_fd,
                0,
            ) {
                Ok(p) => p,
                Err(_) => panic!("Could not resize the shm pool"),
            }
        };
    }
}

impl Drop for ShmRegion {
    fn drop(&mut self) {
        if !self.sr_raw_ptr.is_null() {
            unsafe {
                // We need to manually unmap this region whenever
                // it goes out of scope. These prevent memory leaks
                mman::munmap(self.sr_raw_ptr, self.sr_size).unwrap();
                unistd::close(self.sr_fd).unwrap();
            }
        }
    }
}

// Handles events for the wl_shm interface
//
// There is essentially only one thing going on here,
// we immediately create a shared memory pool and
// create a wl_shm_pool resource to represent it.
pub fn shm_handle_request(req: wl_shm::Request, shm: wl_shm::WlShm) {
    match req {
        wl_shm::Request::CreatePool { id: pool, fd, size } => {
            // We only handle valid sized pools
            if size <= 0 {
                shm.as_ref()
                    .post_error(wl_shm::Error::InvalidFd as u32, "Invalid Fd".to_string());
            }

            let reg = Rc::new(RefCell::new(ShmRegion::new(fd, size as usize).unwrap()));
            // Register a callback for the wl_shm_pool interface
            pool.quick_assign(|p, r, _| {
                shm_pool_handle_request(r, p.deref().clone());
            });
            // Add our ShmRegion as the private data for the pool
            pool.as_ref().user_data().set(move || reg);
        }
        _ => unimplemented!(),
    }
}

// A buffer in shared memory
//
// This represents a region of memory which
// was carved from a ShmRegion. This struct
// did not allocate the shared memory.
#[allow(dead_code)]
pub struct ShmBuffer {
    // The region this buffer is a part of
    sb_reg: Rc<RefCell<ShmRegion>>,
    // The offset into sb_reg where this is located
    sb_offset: i32,
    pub sb_width: i32,
    pub sb_height: i32,
    sb_stride: i32,
    sb_format: wl_shm::Format,
}

impl ShmBuffer {
    // Convert a ShmBuffer to a MemImage
    //
    // subsystems use MemImage to represent raw pointers
    // to memory. We need to find the raw pointer at
    // the correct offset into the region and return
    // it as a MemImage
    pub fn get_mem_image(&self) -> MemImage {
        MemImage::new(
            unsafe {
                self.sb_reg
                    .borrow()
                    .sr_raw_ptr
                    .offset(self.sb_offset as isize)
            } as *mut u8,
            4, // 4 bytes per pixel hardcoded
            self.sb_width as usize,
            self.sb_height as usize,
        )
    }
}

// Handle events for the wl_shm_pool interface
//
// The shared memory pool is going to handle creation of
// buffers, we will carve out a portion of the shared
// memory region to supply one.
pub fn shm_pool_handle_request(req: wl_shm_pool::Request, pool: wl_shm_pool::WlShmPool) {
    // Get the userdata from this resource
    let reg = pool
        .as_ref()
        .user_data()
        .get::<Rc<RefCell<ShmRegion>>>()
        .unwrap();

    match req {
        #[allow(unused_variables)]
        wl_shm_pool::Request::CreateBuffer {
            // id is actually translated to a buffer by wayland-rs
            id: buffer,
            offset,
            width,
            height,
            stride,
            format,
        } => {
            // Ensure that the requested format is supported
            if format != wl_shm::Format::Xrgb8888 && format != wl_shm::Format::Argb8888 {
                pool.as_ref().post_error(
                    wl_shm::Error::InvalidFormat as u32,
                    format!("SHM format {:?} is not supported.", format),
                );
            }

            let data = ShmBuffer {
                sb_reg: reg.clone(),
                sb_offset: offset,
                sb_width: width,
                sb_height: height,
                sb_stride: stride,
                sb_format: format,
            };
            log::debug!("Created new shm buf with size {}x{}", width, height);

            // We still need to register a void callback
            buffer.quick_assign(|_, _, _| {});
            // Add our buffer priv data to the userdata
            buffer.as_ref().user_data().set(move || data);
        }
        wl_shm_pool::Request::Resize { size } => {
            reg.borrow_mut().resize(size as usize);
        }
        wl_shm_pool::Request::Destroy => {}
        _ => unimplemented!(),
    }
}
