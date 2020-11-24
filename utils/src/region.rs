// Region tracking helpers
//
// Austin Shafer - 2020

/// A rectangular region
///
/// This can be used to track input regions,
/// damage boxes, etc. It is determinined by
/// the corders of a rectangle:
///   r_start: the upper left corner's position on the desktop
///   r_size:  the distance from the left to the lower right
#[derive(Debug,PartialEq)]
pub struct Rect<T> {
    pub r_pos: (T, T),
    pub r_size: (T, T),
}

impl<T> Rect<T> {
    pub fn new(x: T, y: T, width: T, height: T)
               -> Rect<T>
    {
        Rect {
            r_pos: (x, y),
            r_size: (width, height),
        }
    }
}
