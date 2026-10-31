//! Shared target-aware C compilation for embedded and Cargo runtime builds.
/// cc-rs discovers the native MSVC environment as well as target-specific
/// CC/AR overrides, and selects the appropriate compiler and archive syntax.
pub fn build(out: &std::path::Path) -> cc::Build {
    let mut build = cc::Build::new();
    build
        .out_dir(out)
        .cargo_metadata(false)
        .opt_level(2)
        .debug(false)
        .warnings(false)
        .pic(true)
        .flag_if_supported("-fvisibility=hidden");
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    build.flag_if_supported(format!("-ffile-prefix-map={root}=/plenty"));
    build
}
