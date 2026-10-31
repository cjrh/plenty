//! The same pinned C codec for Cargo runtime tests and the embedded runtime.
use std::path::{Path, PathBuf};
use std::process::Command;

pub const ARCHIVE: &str = "plenty_brotli";

pub fn build(native: &Path, out: &Path) {
    let source = native.join("brotli-1.2.1");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=AR");
    let cc = crate::native_tools::tool("CC");
    let mut objects = Vec::new();
    for section in ["common", "dec", "enc"] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(source.join("c").join(section))
            .expect("read pinned Brotli sources")
            .map(|entry| entry.expect("read Brotli source entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
            .collect();
        files.sort();
        for file in files {
            let stem = file.file_stem().unwrap().to_str().unwrap();
            let object = out.join(format!("brotli_{section}_{stem}.o"));
            let result = Command::new(&cc)
                .args(["-c", "-O2", "-fPIC", "-g0", "-std=c99", "-w"])
                .args([
                    "-ffunction-sections",
                    "-fdata-sections",
                    "-fvisibility=hidden",
                ])
                .arg(format!(
                    "-ffile-prefix-map={}=/plenty/brotli",
                    source.display()
                ))
                .arg("-DBROTLI_HAVE_LOG2=1")
                // Upstream defaults to exit(EXIT_FAILURE) on allocation
                // failure unless this recoverable memory manager is selected.
                .arg("-DBROTLI_ENCODER_CLEANUP_ON_OOM")
                .arg("-I")
                .arg(source.join("c/include"))
                .arg(&file)
                .arg("-o")
                .arg(&object)
                .output()
                .expect("invoke C compiler for Brotli");
            assert!(
                result.status.success(),
                "compiling {} failed:\n{}",
                file.display(),
                String::from_utf8_lossy(&result.stderr)
            );
            objects.push(object);
        }
    }
    let archive = out.join(format!("lib{ARCHIVE}.a"));
    let _ = std::fs::remove_file(&archive);
    let result = crate::native_tools::archive_command()
        .arg(archive)
        .args(objects)
        .output()
        .expect("archive Brotli");
    assert!(
        result.status.success(),
        "archiving Brotli failed:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
