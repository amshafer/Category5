// A list of surfaces to be displayed
//
// Austin Shafer - 2020

use super::surface::Surface;
use crate::Damage;
use std::iter::DoubleEndedIterator;
use std::ops::Index;

pub struct SurfaceList {
    /// This will get cleared during Thundr::draw
    pub(crate) l_changed: bool,
    l_vec: Vec<Surface>,
    /// List of damage caused by removing/adding surfaces
    pub(crate) l_damage: Vec<Damage>,
}

impl SurfaceList {
    pub fn new() -> Self {
        Self {
            l_changed: false,
            l_vec: Vec::new(),
            l_damage: Vec::new(),
        }
    }

    fn damage_removed_surf(&mut self, mut surf: Surface) {
        surf.record_damage();
        match surf.take_surface_damage() {
            Some(d) => self.l_damage.push(d),
            None => {}
        };
    }

    pub fn remove_surface(&mut self, surf: Surface) {
        let index = match self.l_vec.iter().enumerate().find(|(_, s)| **s == surf) {
            Some((i, _)) => i,
            None => return,
        };
        self.damage_removed_surf(surf);

        self.l_changed = true;
        self.l_vec.remove(index);
    }

    pub fn insert(&mut self, order: usize, mut surf: Surface) {
        self.l_changed = true;
        surf.record_damage();
        self.l_vec.insert(order, surf);
    }

    pub fn push(&mut self, mut surf: Surface) {
        self.l_changed = true;
        surf.record_damage();
        self.l_vec.push(surf);
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Surface> {
        self.l_vec.iter()
    }

    pub fn damage(&self) -> impl DoubleEndedIterator<Item = &Damage> {
        self.l_damage.iter()
    }

    /// This is the generic map implementation, entrypoint to the recursive
    /// surface evaluation.
    #[allow(dead_code)]
    fn map_on_all_surfaces<F>(&self, mut func: F)
    where
        F: FnMut(&Surface) -> bool,
    {
        for surf in self.l_vec.iter() {
            for sub in surf.s_internal.borrow().s_subsurfaces.iter() {
                if !func(sub) {
                    return;
                }
            }
            if !func(surf) {
                return;
            }
        }
    }

    pub fn clear_damage(&mut self) {
        self.l_damage.clear();
    }

    pub fn clear(&mut self) {
        self.l_changed = true;
        // Get the damage from all removed surfaces
        for mut surf in self.l_vec.drain(..) {
            surf.record_damage();
            match surf.take_surface_damage() {
                Some(d) => self.l_damage.push(d),
                None => {}
            };
        }
    }

    pub fn len(&self) -> u32 {
        self.l_vec.len() as u32
    }
}

impl Index<usize> for SurfaceList {
    type Output = Surface;

    fn index(&self, index: usize) -> &Self::Output {
        &self.l_vec[index]
    }
}
