//! # Wayland Server
//!
//! The files here implement the different wayland protocols we
//! support. Usually the filename is a shortened version of the protocol
//! name.
//!
//! ## Design
//!
//! In wayland, code for interacting with the protocol is generated by the
//! `wayland-scanner` program. Code for the core wayland protocols is
//! contained in `libwayland.so`, but for independent protocols we need to
//! generate the code ourselves. This takes place in our `build.rs`, which
//! generates code in the `protocol` directory. Generated code is named
//! `*_generated.rs`. It is then wrapped in a similarly named (but not
//! autogenerated) file which is advertised as a module. See `build.rs`
//! for more.
//!
//! Our wayland singleton and globals are created in the `compositor.rs`
//! file. Effectively, that is the "main" file in this directory. The
//! wayland compositor, registry, and display are created, then a global
//! object advertising each supported protocol is registered.
//!
//! All resources are kept in an entity-component set called the
//! `Atmosphere`. The atmosphere assigns an id to wayland surfaces and
//! wayland clients (WindowId/ClientId). This id can be used to look up
//! properties about that object that are stored in the atmosphere. Most
//! protocol implementations have a pointer to the global atmosphere that
//! they will update. Other subsystems can then see the changes that the
//! protocol handler has made and react accordingly.
//!
//! ## Wayland api
//!
//! Wayland is very callback-driven, so we create a series of handlers
//! that react to requests from clients and update our compositor's
//! state. This is done using our wayland wrapper: the wayland-server
//! crate (which is part of the smithay project).
//!
//! Category5 does not use a high level wayland library such as wlroots or
//! smithay for a few reasons:
//! * It reduces the dependency count.
//! * One of our primary goals is to create a system which is easy to read
//! and hack on. Having the code be half wlroots and half category5 would
//! increase the code (and languages) that a new developer must read, and
//! means that changes have to be contributed to multiple places.
//! * To give category5 more control over itself. We are isolated from the
//! changes in the smithay/wlroots ecosystems. We can choose how we
//! implement every last detail.
//!
//! wayland-server provides enough of a wrapper around the wayland api to
//! make programming easy, but not so much that it gets in our way.

// Austin Shafer - 2019

// Supported protocols
pub mod compositor;
mod data_devices;
mod keyboard;
pub mod linux_dmabuf;
mod pointer;
pub mod protocol;
pub mod seat;
pub mod shm;
pub mod surface;
mod wl_drm;
mod wl_output;
pub mod wl_region;
mod wl_shell;
mod wl_subcompositor;
pub mod xdg_shell;

// Utils
pub mod role;
pub mod task;
pub mod utils;
