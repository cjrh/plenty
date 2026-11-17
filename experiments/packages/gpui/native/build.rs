use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PLENTY_GPUI_APP_DIR");
    if env::var_os("CARGO_FEATURE_APPLICATION").is_none() {
        return;
    }
    let path = PathBuf::from(env::var_os("PLENTY_GPUI_APP_DIR").expect(
        "build the Plenty application first and set PLENTY_GPUI_APP_DIR; see the package README",
    ));
    let path = path.canonicalize().expect("application directory");
    for name in ["libgpui_app.so", "gpui_app.h", "gpui_app.link-args.txt"] {
        println!("cargo:rerun-if-changed={}", path.join(name).display());
    }
    cc::Build::new()
        .file("app.c")
        .include(&path)
        .warnings_into_errors(true)
        .flag("-std=c11")
        .compile("gpui_app_adapter");
    println!("cargo:rerun-if-changed=app.c");
    println!("cargo:rustc-link-search=native={}", path.display());
    println!("cargo:rustc-link-lib=dylib=gpui_app");
    // Outputs retain the package layout: native/target/{debug,release} and
    // target/{optimized,fast-compile}. No global library path is modified.
    let profile = path
        .file_name()
        .expect("application build profile")
        .to_str()
        .expect("UTF-8 profile");
    println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../../../target/{profile}");
    let args =
        fs::read_to_string(path.join("gpui_app.link-args.txt")).expect("native link arguments");
    assert!(
        args.is_empty(),
        "shared application should require no extra link arguments"
    );
}
