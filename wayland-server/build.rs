// Generate bindings the the C libthreadpool on the fly
//   based on the bindgen manual
//
// Austin Shafer - 2019

extern crate bindgen;

fn main () {
    // tell cargo to link with libthreadpool.so
    println!("cargo:rustc-env=LD_LIBRARY_PATH=/usr/local/lib/");
    println!("cargo:rustc-link-search=native=/usr/local/lib/");
    println!("cargo:rustc-link-lib=dylib=wayland-server");

    // update the generated bindings if the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .rustfmt_bindings(true)
        .whitelist_function("wl_.*")
        .whitelist_type("wl_.*")
        .whitelist_var("wl_.*")
        .generate()
        .expect("Could not generate bindings for libthreadpool");

    bindings.write_to_file("src/wayland_bindings.rs")
        .expect("Could not write bindings to ./wayland_bindings.rs");
}
