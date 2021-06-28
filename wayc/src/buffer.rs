use std::cell::RefCell;
use std::rc::Rc;

/// Defines what type of handle this buffer is.
pub enum BufferType {
    /// This buffer is backed by shared memory
    Shm,
    /// This buffer is backed by a dmabuf handle
    DmaBuf,
}

pub type BufferHandle = Rc<RefCell<Buffer>>;

/// A buffer is a set of memory used to define the contents of a surface
pub struct Buffer {
    b_size: (usize, usize),
    b_type: BufferType,
}
