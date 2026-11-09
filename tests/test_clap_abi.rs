//! Compose the C boundary features against pinned native CLAP declarations.
//! This exports an entry and private ABI-test factory, not an audio plugin.
#![cfg(all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"))]
use object::{Object, ObjectSymbol};
use std::path::Path;
use std::process::Command;

#[test]
fn clap_entry_event_callbacks_and_real_unload() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for optimize in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for (from, to) in [
            ("native.plentyi", "native.plentyi"),
            ("library.plenty", "library.plenty"),
        ] {
            std::fs::copy(fixtures.join("clap-abi").join(from), root.join(to)).unwrap();
        }
        let library = root.join("fixture.clap");
        let mut options = plenty::LibraryOptions::new("fixture", plenty::LibraryKind::Shared);
        options.compile.optimize = optimize;
        options.unloadable = true;
        options.compile.link_args.push("-Wl,--gc-sections".into());
        let artifacts =
            plenty::compile_file_to_library(&root.join("library.plenty"), &library, None, &options)
                .unwrap();
        let contract = std::fs::read_to_string(artifacts.interface).unwrap();
        assert!(contract.contains("# C object: clap_entry; size 40; align 8;"));
        assert!(contract.contains("# unloadable-profile: linux-gnu-x86_64-v1"));
        assert!(contract.contains("# Requires the foreign host protocol callback context"));
        let bytes = std::fs::read(&library).unwrap();
        let object = object::File::parse(bytes.as_slice()).unwrap();
        let entry = object
            .dynamic_symbols()
            .find(|symbol| symbol.name().ok() == Some("clap_entry"))
            .expect("CLAP entry must survive unused accessor pruning and linker GC");
        assert_eq!(entry.kind(), object::SymbolKind::Data);
        assert_eq!(entry.size(), 40);
        assert!(entry.is_global());

        let executable = root.join("host");
        let output = Command::new("cc")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2", "-pthread"])
            .arg("-I")
            .arg(fixtures.join("clap-headers/include"))
            .arg(fixtures.join("clap-abi/host.c"))
            .args(["-ldl", "-o"])
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(executable).arg(library).output().unwrap();
        assert!(output.status.success(), "{output:?}");
    }
}
