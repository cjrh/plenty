use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PLENTY_ICED_APP_DIR");
    if env::var_os("CARGO_FEATURE_APPLICATION").is_none() {
        return;
    }
    let path = PathBuf::from(env::var_os("PLENTY_ICED_APP_DIR").expect(
        "build the Plenty application first and set PLENTY_ICED_APP_DIR; see the package README",
    ));
    let path = path.canonicalize().expect("application directory");
    println!(
        "cargo:rerun-if-changed={}",
        path.join("iced_app.h").display()
    );
    let (adapter, guard) = if env::var_os("CARGO_FEATURE_LEGACY_APPLICATION").is_some() {
        ("legacy/app.c", include_str!("legacy/expected-contract.txt"))
    } else {
        ("app.c", include_str!("expected-contract.txt"))
    };
    cc::Build::new()
        .file(adapter)
        .define("REQUIRE_ICED_APP_CONTRACT", guard.trim())
        .include(&path)
        .warnings_into_errors(true)
        .flag("-std=c11")
        .compile("iced_app_adapter");
    println!("cargo:rerun-if-changed={adapter}");
}
