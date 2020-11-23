// Handle imports for the generated wayland bindings
//
// Austin Shafer - 2020
#![allow(dead_code,non_camel_case_types,unused_unsafe,unused_variables)]
#![allow(non_upper_case_globals,non_snake_case,unused_imports)]
extern crate wayland_server;
extern crate wayland_commons;

pub(crate) use wayland_server::sys;
pub(crate) use wayland_server::{Main, Resource, ResourceMap, AnonymousObject};
pub(crate) use wayland_server::protocol::*;

pub(crate) use wayland_commons::map::{Object, ObjectMetadata};
pub(crate) use wayland_commons::{Interface, MessageGroup};
pub(crate) use wayland_commons::wire::{
    Argument, MessageDesc, ArgumentType, Message
};
pub(crate) use wayland_commons::smallvec;

include!("linux_dmabuf_generated.rs");
