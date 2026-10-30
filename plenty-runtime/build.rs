#[path = "native/brotli_build.rs"]
mod brotli_build;
#[path = "native/tools.rs"]
mod native_tools;

fn main() {
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    brotli_build::build(&root.join("native"), &out);
    println!("cargo:rustc-link-search=native={}", out.display());
}
