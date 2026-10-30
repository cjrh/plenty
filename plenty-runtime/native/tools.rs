//! Resolve tools for the Cargo target, never silently use host cc in a cross build.
use std::ffi::OsString;

pub fn tool(kind: &str) -> OsString {
    let target = std::env::var("TARGET").expect("Cargo TARGET");
    let host = std::env::var("HOST").expect("Cargo HOST");
    for key in [
        format!("{kind}_{target}"),
        format!("{kind}_{}", target.replace('-', "_")),
        format!("TARGET_{kind}"),
        kind.to_owned(),
    ] {
        println!("cargo:rerun-if-env-changed={key}");
        if let Some(value) = std::env::var_os(key) {
            return value;
        }
    }
    if target == host {
        return if kind == "CC" { "cc" } else { "ar" }.into();
    }
    let prefix = match target.as_str() {
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        _ => panic!("set {kind}_{target} explicitly for this cross target"),
    };
    format!("{prefix}-{}", if kind == "CC" { "gcc" } else { "ar" }).into()
}
