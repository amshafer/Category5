// Build helper for thundr

fn main() {
    // Search /usr/local/lib/ for libvulkan
    // This is needed on FreeBSD as ash doesn't do this by default
    println!(r"cargo:rustc-link-search=/usr/local/lib/");
}
