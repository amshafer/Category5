// Handle imports for the generated wayland bindings
//
// Austin Shafer - 2021
#![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
#![allow(non_upper_case_globals, non_snake_case, unused_imports)]
extern crate wayland_commons;
extern crate wayland_server;

pub(crate) use wayland_server::protocol::*;
pub(crate) use wayland_server::sys;
pub(crate) use wayland_server::{AnonymousObject, Main, Resource, ResourceMap};

pub(crate) use wayland_commons::map::{Object, ObjectMetadata};
pub(crate) use wayland_commons::smallvec;
pub(crate) use wayland_commons::wire::{Argument, ArgumentType, Message, MessageDesc};
pub(crate) use wayland_commons::{Interface, MessageGroup};

include!("wl_drm_generated.rs");
