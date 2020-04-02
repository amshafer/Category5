// A set of helper structs for common operations
//
// Austin Shafer - 2020

use std::slice;
use std::ops::Deref;

// Represents a raw pointer to a region of memory
// containing an image buffer
//
// *Does Not* free the memory when it is dropped. This
// is used to represent shm buffers from wayland.
pub struct MemImage {
    ptr: *mut u8,
    // size of the pixel elements, in bytes
    pub element_size: usize,
    pub width: usize,
    pub height: usize,
}

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
