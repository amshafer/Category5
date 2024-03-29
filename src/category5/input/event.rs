// Input event representation
//
// Austin Shafer - 2020
extern crate input;
extern crate wayland_server as ws;
use input::event::keyboard::KeyState;
use input::event::pointer::ButtonState;

use ws::protocol::wl_keyboard;

// This is our internal representation of input
//
// It is translated from libinput, and allows us to map
// keycodes to any input without modifying other subsystems
#[derive(Debug)]
pub enum InputEvent {
    pointer_move(PointerMove),
    click(Click),
    key(Key),
    axis(Axis),
}

// Movement of the pointer relative to
// the previous position
#[derive(Debug)]
pub struct PointerMove {
    pub pm_dx: f64,
    pub pm_dy: f64,
}

// Pressing or unpressing a mouse button
#[derive(Debug)]
pub struct Click {
    // from the codes mod
    pub c_code: u32,
    pub c_state: ButtonState,
}

/// Represents a scrolling motion in one of two directions
#[derive(Debug)]
pub struct Axis {
    /// This axis event has a horizontal value
    pub a_has_horiz: bool,
    /// This axis event has a vertical value
    pub a_has_vert: bool,
    /// horizontal motion
    pub a_hori_val: f64,
    /// vertical motion
    pub a_vert_val: f64,
    /// The v120 libinput API value, if it was available
    /// This should only be set on AXIS_SOURCE_WHEEL input devices
    /// (horizontal, vertical)
    pub a_v120_val: (f64, f64),
    /// The axis source.
    pub a_source: u32,
}

pub const AXIS_SOURCE_WHEEL: u32 = 0;
pub const AXIS_SOURCE_FINGER: u32 = 1;

// represents using the keyboard
#[derive(Debug)]
pub struct Key {
    pub k_code: u32,
    pub k_state: KeyState,
}

// A helper function to map a KeyState from the input event
// into a KeyState from wl_keyboard
pub fn map_key_state(state: KeyState) -> wl_keyboard::KeyState {
    match state {
        KeyState::Pressed => wl_keyboard::KeyState::Pressed,
        KeyState::Released => wl_keyboard::KeyState::Released,
    }
}
