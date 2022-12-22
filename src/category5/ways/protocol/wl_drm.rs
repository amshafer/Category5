// Handle imports for the generated wayland bindings
//
// Austin Shafer - 2022
use wayland_scanner;
use wayland_server;
use wayland_server::protocol::*;

// From the wayland_scanner docs

// This module hosts a low-level representation of the protocol objects
// you will not need to interact with it yourself, but the code generated
// by the generate_client_code! macro will use it
pub mod __interfaces {
    // import the interfaces from the core protocol if needed
    use wayland_server::protocol::__interfaces::*;
    wayland_scanner::generate_interfaces!("src/category5/ways/protocol/wayland-drm.xml");
}
use self::__interfaces::*;

// This macro generates the actual types that represent the wayland objects of
// your custom protocol
wayland_scanner::generate_server_code!("src/category5/ways/protocol/wayland-drm.xml");
