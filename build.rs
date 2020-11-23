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

    let out_dir_str = "src/category5/ways/protocol/";
    let protocols_base = "/usr/local/share/wayland-protocols/";
    // The protocols to generate bindings for
    //
    // These paths are relative to protocols_base.
    let protocols = vec![
        "unstable/linux-dmabuf/linux-dmabuf-unstable-v1.xml",
        "stable/xdg-shell/xdg-shell.xml",
    ];
    // These are the names to be used when generating
    // binding files.
    let protocol_names = vec![
        "linux_dmabuf",
        "xdg_shell",
    ];

    for (i, p) in protocols.iter().enumerate() {
        let protocol_file = format!("{}{}",
                                    protocols_base,
                                    p);

        // Target directory for the generate files
        let out_dir = Path::new(&out_dir_str);

        generate_code(
            protocol_file,
            out_dir.join(format!("{}{}", protocol_names[i], "_generated.rs")),
            Side::Server, // Generate server-side code
        );
    }
}
