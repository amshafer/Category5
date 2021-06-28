use crate::wc::protocol::wl_surface::*;
use crate::wc::Main;
use crate::{Buffer, Role};

use std::cell::RefCell;
use std::rc::Rc;

pub type SurfaceHandle = Rc<RefCell<Surface>>;

pub struct Surface {
    s_wl_surf: Main<WlSurface>,
    s_role: Role,
    s_buffer: Option<Buffer>,
}

impl Surface {
    pub fn new(surf: Main<WlSurface>) -> SurfaceHandle {
        Rc::new(RefCell::new(Self {
            s_wl_surf: surf,
            s_role: Role::Unassigned,
            s_buffer: None,
        }))
    }
}
