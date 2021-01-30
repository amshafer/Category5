// A list of surfaces to be displayed
//
// Austin Shafer - 2020

use super::surface::Surface;
use std::iter::DoubleEndedIterator;

pub struct SurfaceList {
    /// This will get cleared during Thundr::draw
    pub(crate) l_changed: bool,
    l_vec: Vec<Surface>,
}

impl SurfaceList {
    pub fn new() -> Self {
        Self {
            l_changed: false,
            l_vec: Vec::new(),
        }
    }

    pub fn remove_surface(&mut self, surf: Surface) {
        let index = match self.l_vec.iter().enumerate().find(|(_, s)| **s == surf) {
            Some((i, _)) => i,
            None => return,
        };

        self.l_changed = true;
        self.l_vec.remove(index);
    }

    pub fn insert_surface_at(&mut self, surf: Surface, order: usize) {
        self.l_changed = true;
        self.l_vec.insert(order, surf);
    }

    pub fn push(&mut self, surf: Surface) {
        self.l_changed = true;
        self.l_vec.push(surf);
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Surface> {
        self.l_vec.iter()
    }

    pub fn clear(&mut self) {
        self.l_changed = true;
        self.l_vec.clear();
    }

    pub fn len(&self) -> u32 {
        self.l_vec.len() as u32
    }
}
