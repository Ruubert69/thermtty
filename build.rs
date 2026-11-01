fn main() {
    let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("thermtty.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    // Embed our manifest (requireAdministrator + PerMonitorV2 DPI) instead of the linker default.
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
}
