use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=PLENTY_EGUI_APP_DIR");
    if env::var_os("CARGO_FEATURE_APPLICATION").is_none() {
        return;
    }
    let path = PathBuf::from(env::var_os("PLENTY_EGUI_APP_DIR").expect(
        "build the Plenty application first and set PLENTY_EGUI_APP_DIR; see the package README",
    ));
    let path = path.canonicalize().expect("application directory");
    println!(
        "cargo:rerun-if-changed={}",
        path.join("egui_app.h").display()
    );
    cc::Build::new()
        .file("app.c")
        .include(&path)
        .warnings_into_errors(true)
        .flag("-std=c11")
        .compile("egui_app_adapter");
    println!("cargo:rerun-if-changed=app.c");
}
