use plenty::{LibraryKind, LibraryOptions};
use std::path::Path;
use std::process::Command;

const SOURCE: &str = "export def answer() -> i32 = \"calc_answer\":\n    42\n";

fn artifacts(root: &Path) -> Vec<std::path::PathBuf> {
    ["calc.h", "calc.plentyi", "calc.link-args.txt", "library"]
        .iter()
        .map(|name| root.join(name))
        .collect()
}

fn no_temporary_directories(root: &Path) {
    assert!(!std::fs::read_dir(root).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".plenty-")));
}

#[test]
fn library_publication_preserves_previous_artifacts_on_failure() {
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("output with, spaces");
        std::fs::create_dir(&root).unwrap();
        let source = root.join("source.plenty");
        let library = root.join("library");
        std::fs::write(&source, SOURCE).unwrap();
        let options = LibraryOptions::new("calc", kind);
        plenty::compile_file_to_library(&source, &library, None, &options).unwrap();
        let paths = artifacts(&root);
        let previous: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
        plenty::verify_library_interface(&library, &paths[1]).unwrap();
        // Invalid later destination must not leave an updated header/interface.
        std::fs::remove_file(&paths[2]).unwrap();
        std::fs::create_dir(&paths[2]).unwrap();
        std::fs::write(&source, SOURCE.replace("i32", "i64")).unwrap();
        let error = plenty::compile_file_to_library(&source, &library, None, &options)
            .unwrap_err()
            .to_string();
        assert!(error.contains("regular file"), "{error}");
        for index in [0, 1, 3] {
            assert_eq!(std::fs::read(&paths[index]).unwrap(), previous[index]);
        }
        std::fs::remove_dir(&paths[2]).unwrap();
        std::fs::write(&paths[2], &previous[2]).unwrap();
        // A broken archiver/linker also leaves the full previous set intact.
        let mut broken = options.clone();
        broken.archiver = "false".into();
        broken.compile.linker = "false".into();
        assert!(plenty::compile_file_to_library(&source, &library, None, &broken).is_err());
        for (path, bytes) in paths.iter().zip(&previous) {
            assert_eq!(std::fs::read(path).unwrap(), *bytes);
        }
        no_temporary_directories(&root);
        // Successful replacement, including a version-script path containing a comma.
        plenty::compile_file_to_library(&source, &library, None, &options).unwrap();
        plenty::verify_library_interface(&library, &paths[1]).unwrap();
        assert!(std::fs::read_to_string(&paths[0])
            .unwrap()
            .contains("int64_t calc_answer"));
        no_temporary_directories(&root);
    }
}

#[test]
fn protects_imported_sources_and_rejects_symlink_destinations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let source = root.join("source.plenty");
    let library = root.join("library");
    let imported = root.join("calc.plentyi");
    let declaration = "pub def value() -> i32:\n    42\n";
    std::fs::write(&imported, declaration).unwrap();
    std::fs::write(
        &source,
        "import calc\nexport def answer() -> i32 = \"calc_answer\":\n    calc.value()\n",
    )
    .unwrap();
    let options = LibraryOptions::new("calc", LibraryKind::Static);
    let error = plenty::compile_file_to_library(&source, &library, None, &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("overwrite an input"), "{error}");
    assert_eq!(std::fs::read_to_string(&imported).unwrap(), declaration);
    assert!(!library.exists());
    std::fs::write(&source, SOURCE).unwrap();
    std::fs::remove_file(&imported).unwrap();
    let target = root.join("untouched");
    std::fs::write(&target, "user contents").unwrap();
    std::os::unix::fs::symlink(&target, root.join("calc.h")).unwrap();
    assert!(
        plenty::compile_file_to_library(&source, &library, None, &options)
            .unwrap_err()
            .to_string()
            .contains("symlink")
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "user contents");
    std::fs::remove_file(root.join("calc.h")).unwrap();
    plenty::compile_file_to_library(&source, &library, None, &options).unwrap();
    let output = root.join("extract.plentyi");
    std::os::unix::fs::symlink(&target, &output).unwrap();
    assert!(plenty::extract_library_interface(&library, "calc", &output).is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "user contents");
    no_temporary_directories(root);
}

#[test]
fn cli_preserves_linker_arguments_with_comma_containing_temporary_paths() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("with,comma");
    std::fs::create_dir(&root).unwrap();
    let source = root.join("source.plenty");
    std::fs::write(&source, SOURCE).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_plenty"))
        .arg("--shared-library")
        .arg(&source)
        .args(["--library-name", "calc", "-o"])
        .arg(root.join("library.so"))
        .env("TMPDIR", &root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    no_temporary_directories(&root);
}
