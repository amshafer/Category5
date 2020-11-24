// A list of surfaces to be displayed
//
// Austin Shafer - 2020

use super::surface::Surface;
use super::damage::Damage;
use std::iter::Iterator;

pub struct SurfaceList {
    l_vec: Vec<Surface>,
}

impl SurfaceList {
    pub fn new() -> Self {
        Self {
            l_vec: Vec::new(),
        }
    }

    pub fn remove_surface(&mut self, surf: Surface) {
        let index = match self.l_vec.iter().enumerate()
            .find(|(_, s)| **s == surf)
        {
            Some((i, _)) => i,
            None => return,
        };

        self.l_vec.remove(index);
    }

    pub fn insert_surface_at(&mut self, surf: Surface, order: usize) {
        self.l_vec.insert(order, surf);
    }

    pub fn push(&mut self, surf: Surface) {
        self.l_vec.push(surf);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Surface> {
        self.l_vec.iter()
    }

    pub fn clear(&mut self) {
        self.l_vec.clear();
    }
}
