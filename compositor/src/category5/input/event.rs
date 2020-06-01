// Input event representation
//
// Austin Shafer - 2020

// This is our internal representation of input
//
// It is translated from libinput, and allows us to map
// keycodes to any input without modifying other subsystems
pub enum InputEvent {
    pointer_move(PointerMove),
}

// Movement of the pointer relative to
// the previous position
pub struct PointerMove {
    pub pm_dx: f64,
    pub pm_dy: f64,
}
