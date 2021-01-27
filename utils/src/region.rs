// Region tracking helpers
//
// Austin Shafer - 2020

use serde::{Deserialize, Serialize};

use std::cmp::PartialOrd;
use std::ops::Add;

/// A rectangular region
///
/// This can be used to track input regions,
/// damage boxes, etc. It is determinined by
/// the corders of a rectangle:
///   r_start: the upper left corner's position on the desktop
///   r_size:  the distance from the left to the lower right
#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
#[repr(C)]
pub struct Rect<T: PartialOrd + Copy + Add + Add<Output = T>> {
    pub r_pos: (T, T),
    pub r_size: (T, T),
}

impl<T: PartialOrd + Copy + Add + Add<Output = T>> Rect<T> {
    pub fn new(x: T, y: T, width: T, height: T) -> Rect<T> {
        Rect {
            r_pos: (x, y),
            r_size: (width, height),
        }
    }

    /// Checks if the point (x,y) is contained within this
    /// Rectangle.
    pub fn intersects(&self, x: T, y: T) -> bool {
        x > self.r_pos.0
            && y > self.r_pos.1
            && x < self.r_pos.0 + self.r_size.0
            && y < self.r_pos.1 + self.r_size.1
    }
}
