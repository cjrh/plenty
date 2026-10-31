//! Shared target-aware C compilation for embedded and Cargo runtime builds.
/// cc-rs discovers the native MSVC environment as well as target-specific
/// CC/AR overrides, and selects the appropriate compiler and archive syntax.
pub fn build(out: &std::path::Path) -> cc::Build {
    // Suppress link directives: these archives belong to the embedded runtime,
    // not the compiler binary. cc-rs also suppresses environment tracking with
    // cargo_metadata(false), so retain it explicitly for target tool overrides.
    let target = std::env::var("TARGET").unwrap();
    for kind in ["CC", "AR", "CFLAGS", "ARFLAGS"] {
        for key in [
            kind.to_owned(),
            format!("TARGET_{kind}"),
            format!("HOST_{kind}"),
            format!("{kind}_{target}"),
            format!("{kind}_{}", target.replace('-', "_")),
        ] {
            println!("cargo:rerun-if-env-changed={key}");
        }
    }
    for key in ["CRATE_CC_NO_DEFAULTS", "CC_SHELL_ESCAPED_FLAGS"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
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
