// A set of helper structs for common operations
//
// Austin Shafer - 2020
extern crate wayland_server as ws;

use ws::protocol::wl_buffer;

use std::{slice,fmt};
use std::ops::Deref;
use std::os::unix::io::RawFd;

pub enum WindowContents<'a> {
    dmabuf(&'a Dmabuf),
    mem_image(&'a MemImage),
}

#[derive(Debug)]
pub enum ReleaseInfo {
    none,
    mem_image,
    dmabuf(DmabufReleaseInfo),
}

// Represents a raw pointer to a region of memory
// containing an image buffer
//
// *Does Not* free the memory when it is dropped. This
// is used to represent shm buffers from wayland.
#[derive(Debug)]
pub struct MemImage {
    ptr: *mut u8,
    // size of the pixel elements, in bytes
    pub element_size: usize,
    pub width: usize,
    pub height: usize,
}

#[allow(dead_code)]
impl MemImage {
    pub fn as_slice(&self) -> &[u8] {
        if !self.ptr.is_null() {
            unsafe {
                return slice::from_raw_parts(
                    self.ptr,
                    self.width * self.height * self.element_size,
                );
            }
        } else {
            panic!("Trying to dereference null pointer");
        }
    }

    pub fn new(ptr: *mut u8,
               element_size: usize,
               width: usize,
               height: usize)
               -> MemImage
    {
        MemImage {
            ptr: ptr,
            element_size: element_size,
            width: width,
            height: height,
        }
    }
}

// While it is safe according to the language, it is not actually
// safe to use. This is needed so that a MemImage can be sent from
// the wayland thread to the rendering thread. The rendering thread
// needs to consume this immediately. If the wl_buffer is released
// before this is consumed then things will become very bad.
unsafe impl Send for MemImage {}

impl Deref for MemImage {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        if !self.ptr.is_null() {
            return self.as_slice();
        } else {
            panic!("Trying to dereference null pointer");
        }
    }
}

// dmabuf from linux_dmabuf
// Represents one dma buffer the client has added.
// Will be referenced by Params during wl_buffer
// creation.
#[allow(dead_code)]
#[derive(Debug,Copy,Clone)]
pub struct Dmabuf {
    pub db_fd: RawFd,
    pub db_plane_idx: u32,
    pub db_offset: u32,
    pub db_stride: u32,
    // These will be added later during creation
    pub db_width: i32,
    pub db_height: i32,
}

pub struct DmabufReleaseInfo {
    pub dr_wl_buffer: wl_buffer::WlBuffer,
}

impl Drop for DmabufReleaseInfo {
    fn drop(&mut self) {
        println!("Deleting wl_buffer for a dmabuf");
        self.dr_wl_buffer.release();
    }
}

impl fmt::Debug for DmabufReleaseInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DmabufReleaseInfo")
         .field("dr_wl_buffer", &"<wl_buffer omitted>".to_string())
         .finish()
    }
}

impl Dmabuf {
    pub fn new(fd: RawFd,
               plane: u32,
               offset: u32,
               stride: u32)
               -> Dmabuf
    {
        Dmabuf {
            db_fd: fd,
            db_plane_idx: plane,
            db_offset: offset,
            db_stride: stride,
            // these will be added later during creation
            db_width: -1,
            db_height: -1,
        }
    }
}
