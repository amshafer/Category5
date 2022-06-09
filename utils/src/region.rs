// Region tracking helpers
//
// Austin Shafer - 2020

use serde_derive::{Deserialize, Serialize};

use std::cmp::{Ord, PartialOrd};
use std::ops::{Add, Sub};

/// A rectangular region
///
/// This can be used to track input regions,
/// damage boxes, etc. It is determinined by
/// the corders of a rectangle:
///   r_start: the upper left corner's position on the desktop
///   r_size:  the distance from the left to the lower right
#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Serialize, Deserialize)]
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

impl<T: Ord + PartialOrd + Copy + Add + Add<Output = T> + Sub + Sub<Output = T>> Rect<T> {
    /// Clip this Rect inside `other`.
    pub fn clip(&self, other: &Rect<T>) -> Rect<T> {
        Rect::new(
            std::cmp::max(self.r_pos.0, other.r_pos.0),
            std::cmp::max(self.r_pos.1, other.r_pos.1),
            std::cmp::min(self.r_size.0, other.r_size.0 - self.r_pos.0),
            std::cmp::min(self.r_size.1, other.r_size.1 - self.r_pos.1),
        )
    }

    /// Enlarge this rect enough to contain `other`
    pub fn union(&mut self, other: &Self) {
        self.r_pos.0 = std::cmp::min(self.r_pos.0, other.r_pos.0);
        self.r_pos.1 = std::cmp::max(self.r_pos.1, other.r_pos.1);
        self.r_size.0 = std::cmp::min(self.r_size.0, other.r_size.0);
        self.r_size.1 = std::cmp::max(self.r_size.1, other.r_size.1);
    }
}

impl From<Rect<f32>> for Rect<i32> {
    fn from(src: Rect<f32>) -> Rect<i32> {
        Rect {
            r_pos: (src.r_pos.0 as i32, src.r_pos.1 as i32),
            r_size: (src.r_size.0 as i32, src.r_size.1 as i32),
        }
    }
}
