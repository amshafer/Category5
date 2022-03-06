// A list of surfaces to be displayed
//
// Austin Shafer - 2020

use super::surface::Surface;
use crate::{Damage, Result, ThundrError};
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

    pub fn remove(&mut self, index: usize) {
        self.l_changed = true;
        let surf = self.l_vec.remove(index);
        self.damage_removed_surf(surf);
    }

    pub fn remove_surface(&mut self, surf: Surface) -> Result<()> {
        let (index, _) = self
            .l_vec
            .iter()
            .enumerate()
            .find(|(_, s)| **s == surf)
            .ok_or(ThundrError::SURFACE_NOT_FOUND)?;
        self.remove(index);

        if let Some(mut parent) = surf.get_parent() {
            parent.remove_subsurface(surf)?;
        }

        Ok(())
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
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut Surface> {
        self.l_vec.iter_mut()
    }
    pub fn damage(&self) -> impl DoubleEndedIterator<Item = &Damage> {
        self.l_damage.iter()
    }

    fn map_per_surf_recurse<F>(&self, func: &mut F, surf: &Surface, x: i32, y: i32) -> bool
    where
        F: FnMut(&Surface, i32, i32) -> bool,
    {
        let internal = surf.s_internal.borrow();
        let surf_pos = &internal.s_rect.r_pos;

        // Note that the subsurface list is "reversed", with the front subsurface
        // being at the end of the array
        for sub in internal.s_subsurfaces.iter().rev() {
            // Add this surfaces offset to the subdsurface calculations.
            if !self.map_per_surf_recurse(func, sub, x + surf_pos.0 as i32, y + surf_pos.1 as i32) {
                return false;
            }
        }
        func(surf, x, y)
    }

    /// This is the generic map implementation, entrypoint to the recursive
    /// surface evaluation.
    pub fn map_on_all_surfaces<F>(&self, mut func: F)
    where
        F: FnMut(&Surface, i32, i32) -> bool,
    {
        for surf in self.l_vec.iter() {
            // Start here at no offset
            if !self.map_per_surf_recurse(&mut func, surf, 0, 0) {
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

    /// The length only considering immediate surfaces in the list
    pub fn len(&self) -> u32 {
        self.l_vec.len() as u32
    }

    /// The length accounting for subsurfaces
    pub fn len_with_subsurfaces(&self) -> u32 {
        let mut count = 0;
        self.map_on_all_surfaces(|_, _, _| {
            count += 1;
            return true;
        });

        count
    }
}

impl Index<usize> for SurfaceList {
    type Output = Surface;

    fn index(&self, index: usize) -> &Self::Output {
        &self.l_vec[index]
    }
}
