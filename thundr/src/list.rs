// A list of surfaces to be displayed
//
// Austin Shafer - 2020

use super::surface::Surface;
use std::iter::DoubleEndedIterator;

use std::cell::RefCell;
use std::rc::Rc;

pub struct SurfaceListInternal {
    /// This will get cleared during Thundr::draw
    pub(crate) l_changed: bool,
    l_vec: Vec<Surface>,
}

impl SurfaceListInternal {
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Surface> {
        self.l_vec.iter()
    }
}

pub struct SurfaceList {
    pub(crate) sl_internal: Rc<RefCell<SurfaceListInternal>>,
}

impl SurfaceList {
    pub fn new() -> Self {
        Self {
            sl_internal: Rc::new(RefCell::new(SurfaceListInternal {
                l_changed: false,
                l_vec: Vec::new(),
            })),
        }
    }

    pub fn remove_surface(&mut self, surf: Surface) {
        let mut internal = self.sl_internal.borrow_mut();

        let index = match internal.l_vec.iter().enumerate().find(|(_, s)| **s == surf) {
            Some((i, _)) => i,
            None => return,
        };

        internal.l_changed = true;
        internal.l_vec.remove(index);
    }

    pub fn insert_surface_at(&mut self, surf: Surface, order: usize) {
        let mut internal = self.sl_internal.borrow_mut();
        internal.l_changed = true;
        internal.l_vec.insert(order, surf);
    }

    pub fn push(&mut self, surf: Surface) {
        let mut internal = self.sl_internal.borrow_mut();
        internal.l_changed = true;
        internal.l_vec.push(surf);
    }

    pub fn is_changed(&self) -> bool {
        self.sl_internal.borrow().l_changed
    }
    pub fn set_changed(&self, changed: bool) {
        self.sl_internal.borrow_mut().l_changed = changed;
    }

    pub fn clear(&mut self) {
        let mut internal = self.sl_internal.borrow_mut();
        internal.l_changed = true;
        internal.l_vec.clear();
    }

    pub fn len(&self) -> u32 {
        self.sl_internal.borrow().l_vec.len() as u32
    }
}
