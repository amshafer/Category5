// Link with libwayland and generate any wayland bindings
//
// Austin Shafer - 2019
fn main() {
    // tell cargo to link with libwayland-server.so
    println!("cargo:rustc-env=LD_LIBRARY_PATH=/usr/local/lib/");
    println!("cargo:rustc-link-search=native=/usr/local/lib/");
    println!("cargo:rustc-link-lib=dylib=wayland-server");
}
