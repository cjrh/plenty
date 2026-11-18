#[path = "native/brotli_build.rs"]
mod brotli_build;
#[path = "native/cc_build.rs"]
mod native_tools;
#[path = "native/sqlite_build.rs"]
mod sqlite_build;

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    if std::env::var_os("CARGO_FEATURE_EXECUTABLE").is_some() {
        println!("cargo:rustc-cfg=plenty_runtime_embedded");
        sqlite_build::build(&root.join("native"), &out);
        println!("cargo:rustc-cfg=plenty_runtime_sqlite");
        println!("cargo:rustc-link-lib=static={}", sqlite_build::ARCHIVE);
    }
    brotli_build::build(&root.join("native"), &out);
    println!("cargo:rustc-link-search=native={}", out.display());
}
