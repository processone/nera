fn main() {
    // Tell cargo about our custom cfg so check-cfg doesn't warn.
    println!("cargo::rustc-check-cfg=cfg(mio_supported)");

    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if matches!(
        target.as_str(),
        "linux"
            | "macos"
            | "freebsd"
            | "netbsd"
            | "openbsd"
            | "dragonfly"
            | "ios"
    ) {
        println!("cargo:rustc-cfg=mio_supported");
    }
}
