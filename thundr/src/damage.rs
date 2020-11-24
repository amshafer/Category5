// Damage tracking for a surface
//
// Austin Shafer - 2020

#[derive(PartialEq)]
pub struct Damage {
    d_damaged: bool,
}

impl Damage {
    pub fn new() -> Self {
        Self {
            d_damaged: true,
        }
    }
}
