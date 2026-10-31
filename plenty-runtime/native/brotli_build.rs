//! The same pinned C codec for Cargo runtime tests and the embedded runtime.
use std::path::{Path, PathBuf};

pub const ARCHIVE: &str = "plenty_brotli";

pub fn build(native: &Path, out: &Path) {
    let source = native.join("brotli-1.2.1");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    let mut build = crate::native_tools::build(out);
    build
        .std("c99")
        .define("BROTLI_HAVE_LOG2", "1")
        // Upstream's recoverable allocator must unwind encoder state on OOM.
        .define("BROTLI_ENCODER_CLEANUP_ON_OOM", None)
        .include(source.join("c/include"));
    for section in ["common", "dec", "enc"] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(source.join("c").join(section))
            .expect("read pinned Brotli sources")
            .map(|entry| entry.expect("read Brotli source entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
            .collect();
        files.sort();
        for file in files {
            build.file(file);
        }
    }
    build.compile(ARCHIVE);
}
