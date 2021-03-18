// Damage tracking for a surface
//
// Austin Shafer - 2020
use utils::region::Rect;

/// Damage is always in surface coord space
#[derive(Debug, PartialEq)]
pub struct Damage {
    pub(crate) d_damaged: bool,
    d_regions: Vec<Rect<i32>>,
}

impl Damage {
    pub fn empty() -> Self {
        Self {
            d_damaged: false,
            d_regions: Vec::with_capacity(0),
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.d_damaged
    }

    pub fn new(regions: Vec<Rect<i32>>) -> Self {
        Self {
            d_damaged: true,
            d_regions: regions,
        }
    }

    pub fn regions(&self) -> impl Iterator<Item = &Rect<i32>> {
        self.d_regions.iter()
    }

    /// Add a region to this damage collection
    pub fn add(&mut self, rect: &Rect<i32>) {
        self.d_damaged = true;
        self.d_regions.push(*rect);
    }

    pub fn union(&mut self, other: &Self) {
        self.d_regions.extend(&other.d_regions);
        if self.d_regions.len() > 0 {
            self.d_damaged = true;
        }
    }
}
