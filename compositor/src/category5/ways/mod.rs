// Wayland binding fun fun fun
//
//
// Austin Shafer - 2019

#![allow(dead_code, unused_variables, non_camel_case_types)]
#[macro_use]
pub mod utils;
#[allow(non_upper_case_globals)]
mod wayland_bindings;
#[macro_use]
mod wayland_safe;
pub mod compositor;
mod surface;
mod wl_shell;

pub mod task;
