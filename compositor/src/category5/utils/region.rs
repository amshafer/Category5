// Region tracking helpers
//
// Austin Shafer - 2020

// Abstraction of an arbitrary 2D position
//
// This an offset of unspecified units from
// some basis. Basically a cartesian point.
pub struct Offset2D {
    pub x: f64,
    pub y: f64,
}

// A rectangular region
//
// This can be used to track input regions,
// damage boxes, etc. It is determinined by
// the corders of a rectangle:
//   r_start: the upper left corner's position on the desktop
//   r_size:  the distance from the left to the lower right
pub struct Rect {
    pub r_start: Offset2D,
    pub r_size: Offset2D,
}
