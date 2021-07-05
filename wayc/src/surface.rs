use crate::wc::protocol::wl_surface::*;
use crate::wc::Main;
use crate::{BufferHandle, Role};

use std::cell::RefCell;
use std::rc::Rc;

pub type SurfaceHandle = Rc<RefCell<Surface>>;

pub struct Surface {
    pub(crate) s_wl_surf: Main<WlSurface>,
    /// The role of the surface determines how it is presented on the screen. It
    /// could be a layered subsurface, or a normal desktop window.
    pub(crate) s_role: Role,
    /// This is the buffer currently backing this surface
    pub(crate) s_buffer: Option<BufferHandle>,
    /// This is the buffer that will be committed next
    pub(crate) s_attached_buffer: Option<BufferHandle>,
}

impl Surface {
    pub fn new(surf: Main<WlSurface>) -> SurfaceHandle {
        Rc::new(RefCell::new(Self {
            s_wl_surf: surf,
            s_role: Role::Unassigned,
            s_attached_buffer: None,
            s_buffer: None,
        }))
    }

    /// Attach or clear a new buffer to back this surface
    pub fn attach(&mut self, buf: Option<BufferHandle>) {
        if let Some(b) = buf.as_ref() {
            self.s_wl_surf.attach(Some(&b.borrow().b_wl_buf), 0, 0);
        } else {
            self.s_wl_surf.attach(None, 0, 0);
        }

        self.s_attached_buffer = buf;
    }

    /// This function commits all pending changes to the compositor,
    /// making all of them visible at once.
    pub fn commit(&mut self) {
        self.s_wl_surf.commit();
    }
}
