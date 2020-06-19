// Input event representation
//
// Austin Shafer - 2020
extern crate input;
use input::event::pointer::ButtonState;

// This is our internal representation of input
//
// It is translated from libinput, and allows us to map
// keycodes to any input without modifying other subsystems
pub enum InputEvent {
    pointer_move(PointerMove),
    left_click(LeftClick),
}

// Movement of the pointer relative to
// the previous position
pub struct PointerMove {
    pub pm_dx: f64,
    pub pm_dy: f64,
}

// Pressing or unpressing a the main mouse button
pub struct LeftClick {
    pub pb_state: ButtonState,
}
