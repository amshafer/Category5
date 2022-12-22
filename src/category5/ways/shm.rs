// Implementation of the wl_shm_interface
//
// Austin Shafer - 2020
//
// Inspired by the shm module in smithay
extern crate nix;
extern crate wayland_server as ws;

use ws::protocol::wl_buffer;
use ws::protocol::{wl_shm, wl_shm_pool};
use ws::Resource;

use crate::category5::Climate;
use utils::{log, MemImage};

use nix::{sys::mman, unistd};
use std::ffi::c_void;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::{Arc, Mutex};

#[allow(unused_variables)]
impl ws::GlobalDispatch<wl_shm::WlShm, ()> for Climate {
    fn bind(
        state: &mut Self,
        handle: &ws::DisplayHandle,
        client: &ws::Client,
        resource: ws::New<wl_shm::WlShm>,
        global_data: &(),
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

// Dispatch<Interface, Userdata>
#[allow(unused_variables)]
impl ws::Dispatch<wl_shm::WlShm, ()> for Climate {
    // Handles events for the wl_shm interface
    //
    // There is essentially only one thing going on here,
    // we immediately create a shared memory pool and
    // create a wl_shm_pool resource to represent it.
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_shm::WlShm,
        request: wl_shm::Request,
        data: &(),
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        match request {
            wl_shm::Request::CreatePool { id, fd, size } => {
                // We only handle valid sized pools
                if size <= 0 {
                    resource.post_error(wl_shm::Error::InvalidFd as u32, "Invalid Fd".to_string());
                }

                let reg = Arc::new(Mutex::new(
                    ShmRegion::new(fd.as_raw_fd(), size as usize).unwrap(),
                ));
                // Add our ShmRegion as the private data for the pool
                data_init.init(id, reg);
            }
            _ => unimplemented!(),
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &(),
    ) {
    }
}

#[allow(unused_variables)]
impl ws::Dispatch<wl_shm_pool::WlShmPool, Arc<Mutex<ShmRegion>>> for Climate {
    // Handle events for the wl_shm_pool interface
    //
    // The shared memory pool is going to handle creation of
    // buffers, we will carve out a portion of the shared
    // memory region to supply one.
    // Get the userdata from this resource
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_shm_pool::WlShmPool,
        request: wl_shm_pool::Request,
        data: &Arc<Mutex<ShmRegion>>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
        match request {
            #[allow(unused_variables)]
            wl_shm_pool::Request::CreateBuffer {
                // id is actually translated to a buffer by wayland-rs
                id,
                offset,
                width,
                height,
                stride,
                format: format_enum,
            } => {
                let format = format_enum.into_result().expect("Not a valid format");

                // Ensure that the requested format is supported
                if format != wl_shm::Format::Xrgb8888 && format != wl_shm::Format::Argb8888 {
                    resource.post_error(
                        wl_shm::Error::InvalidFormat as u32,
                        format!("SHM format {:?} is not supported.", format),
                    );
                }

                let buf = ShmBuffer {
                    sb_reg: data.clone(),
                    sb_offset: offset,
                    sb_width: width,
                    sb_height: height,
                    sb_stride: stride,
                    sb_format: format,
                };
                log::debug!("Created new shm buf with size {}x{}", width, height);

                // Add our buffer priv data to the userdata
                data_init.init(id, Arc::new(buf));
            }
            wl_shm_pool::Request::Resize { size } => {
                data.lock().unwrap().resize(size as usize);
            }
            wl_shm_pool::Request::Destroy => {}
            _ => unimplemented!(),
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &Arc<Mutex<ShmRegion>>,
    ) {
    }
}

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

// Have to do this manually because of the void *
unsafe impl Send for ShmRegion {}

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

// A buffer in shared memory
//
// This represents a region of memory which
// was carved from a ShmRegion. This struct
// did not allocate the shared memory.
#[allow(dead_code)]
pub struct ShmBuffer {
    // The region this buffer is a part of
    sb_reg: Arc<Mutex<ShmRegion>>,
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
        let mut ret = MemImage::new(
            unsafe {
                self.sb_reg
                    .lock()
                    .unwrap()
                    .sr_raw_ptr
                    .offset(self.sb_offset as isize)
            } as *mut u8,
            4, // 4 bytes per pixel hardcoded
            self.sb_width as usize,
            self.sb_height as usize,
        );
        // Need to convert from size in bytes to size
        // in texels as per Vulkan
        ret.set_stride((self.sb_stride / 4) as u32);

        return ret;
    }
}

// Handle buffers with shm attached
#[allow(unused_variables)]
impl ws::Dispatch<wl_buffer::WlBuffer, Arc<ShmBuffer>> for Climate {
    fn request(
        state: &mut Self,
        client: &ws::Client,
        resource: &wl_buffer::WlBuffer,
        request: wl_buffer::Request,
        data: &Arc<ShmBuffer>,
        dhandle: &ws::DisplayHandle,
        data_init: &mut ws::DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        state: &mut Self,
        _client: ws::backend::ClientId,
        _resource: ws::backend::ObjectId,
        data: &Arc<ShmBuffer>,
    ) {
        // don't close shm fd here since it is handled in Drop
    }
}
