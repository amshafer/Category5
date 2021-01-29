// Damage tracking for a surface
//
// Austin Shafer - 2020
use utils::region::Rect;

/// Damage is always in surface coord space
#[derive(PartialEq)]
pub struct Damage {
    pub(crate) d_damaged: bool,
    d_regions: Vec<Rect<i32>>,
}

impl Damage {
    pub fn empty() -> Self {
        Self::new(Vec::new())
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
        self.d_regions.push(*rect);
    }
}
