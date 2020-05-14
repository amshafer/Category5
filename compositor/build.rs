// Link with libwayland and generate any wayland bindings
//
// Austin Shafer - 2019
extern crate wayland_scanner;

use std::path::Path;
use wayland_scanner::{Side, generate_code};

fn main() {
    // tell cargo to link with libwayland-server.so
    println!("cargo:rustc-env=LD_LIBRARY_PATH=/usr/local/lib/");
    println!("cargo:rustc-link-search=native=/usr/local/lib/");
    println!("cargo:rustc-link-lib=dylib=wayland-server");

    // Location of the xml file, relative to the `Cargo.toml`
    let protocol_file = "/usr/local/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml";

    // Target directory for the generate files
    let out_dir_str = "src/category5/ways/protocol/";
    let out_dir = Path::new(&out_dir_str);

    generate_code(
        protocol_file,
        out_dir.join("xdg_shell_generated.rs"),
        Side::Server, // Generate server-side code
    );
}
