// Input event representation
//
// Austin Shafer - 2020
extern crate wayland_server as ws;
extern crate input;
use input::event::pointer::ButtonState;
use input::event::keyboard::KeyState;

use ws::protocol::wl_keyboard;

// This is our internal representation of input
//
// It is translated from libinput, and allows us to map
// keycodes to any input without modifying other subsystems
pub enum InputEvent {
    pointer_move(PointerMove),
    click(Click),
    key(Key),
    axis(Axis),
}

// Movement of the pointer relative to
// the previous position
pub struct PointerMove {
    pub pm_dx: f64,
    pub pm_dy: f64,
}

// Pressing or unpressing a mouse button
pub struct Click {
    // from the codes mod
    pub c_code: u32,
    pub c_state: ButtonState,
}

// Represents a scrolling motion in one of two directions
pub struct Axis {
    // horizontal motion
    pub a_hori_val: f64,
    // vertical motion
    pub a_vert_val: f64,
}

// represents using the keyboard
pub struct Key {
    pub k_code: u32,
    pub k_state: KeyState,
}

// A helper function to map a KeyState from the input event
// into a KeyState from wl_keyboard
pub fn map_key_state(state: KeyState) -> wl_keyboard::KeyState {
    match state {
        KeyState::Pressed =>
            wl_keyboard::KeyState::Pressed,
        KeyState::Released =>
            wl_keyboard::KeyState::Released,
    }
}
