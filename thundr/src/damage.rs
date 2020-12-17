// Damage tracking for a surface
//
// Austin Shafer - 2020
use utils::region::Rect;

/// Damage is always in surface coord space
#[derive(PartialEq)]
pub struct Damage {
    pub(crate) d_damaged: bool,
    pub(crate) d_region: Rect<f32>,
}

impl Damage {
    pub fn new(region: Rect<f32>) -> Self {
        Self {
            d_damaged: true,
            d_region: region,
        }
    }
}
