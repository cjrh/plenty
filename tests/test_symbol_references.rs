//! How generated code refers to runtime helpers, its own data, and native
//! libraries (issue #372): see "Symbol references" in
//! `book/src/design/toolchain/02-architecture/emission.md`.

mod support;
use std::path::Path;
use std::process::Command;
use support::assert_success as success;

const INTERFACE: &str = "pub extern def labs(value: i64) -> i64 = \"labs\"\n";

const PROGRAM: &str = r#"import native

def twice(v: i64) -> i64:
    v * 2

def main() -> Result[(), AllocError | IoError]:
    words = ["a", "bc"]?
    print(twice(native.labs(-4)), words)?
    Ok(())
"#;

fn write_program(dir: &Path) -> std::path::PathBuf {
    std::fs::write(dir.join("native.plentyi"), INTERFACE).unwrap();
    let path = dir.join("main.plenty");
    std::fs::write(&path, PROGRAM).unwrap();
    path
}

/// `(type, symbol)` for each relocation in the object's code sections.
fn code_relocations(object: &Path) -> Vec<(String, String)> {
    let output = success(
        Command::new("readelf")
            .arg("-rW")
            .arg(object)
            .output()
            .unwrap(),
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let mut relocations = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        if let Some(section) = line.strip_prefix("Relocation section '") {
            in_code = section.starts_with(".rela.text");
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if in_code && fields.len() >= 5 && fields[2].starts_with("R_X86_64_") {
            relocations.push((fields[2].to_string(), fields[4].to_string()));
        }
    }
    relocations
}

#[cfg(target_os = "linux")]
#[test]
fn runtime_calls_and_local_data_are_pc_relative_and_native_libraries_use_the_got() {
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let source = write_program(dir.path());
        let object = dir.path().join("main.o");
        let options = plenty::CompileOptions {
            optimize,
            ..Default::default()
        };
        plenty::compile_file_to_object_with_options(&source, &object, None, &options).unwrap();
        let relocations = code_relocations(&object);
        let got: Vec<_> = relocations
            .iter()
            .filter(|(kind, _)| kind.contains("GOT"))
            .map(|(_, symbol)| symbol.as_str())
            .collect();
        // The C library may be shared; the runtime and the object's own data
        // never are.
        assert_eq!(got, ["labs"], "optimize={optimize}: {relocations:?}");
        for helper in ["plenty_op_166", "plenty_release", "plenty_traps_register"] {
            assert!(
                relocations
                    .iter()
                    .any(|(kind, symbol)| kind == "R_X86_64_PLT32" && symbol == helper),
                "optimize={optimize}: no direct call to {helper}: {relocations:?}"
            );
        }
        assert!(
            relocations
                .iter()
                .any(|(kind, symbol)| kind == "R_X86_64_PC32" && symbol.starts_with(".L")),
            "optimize={optimize}: no PC-relative data address: {relocations:?}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn position_independent_executable_links_without_text_relocations() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_program(dir.path());
    let executable = dir.path().join("program");
    // Request PIE explicitly: not every `cc` makes it the default.
    let options = plenty::CompileOptions {
        optimize: true,
        link_args: vec!["-pie".into()],
        ..Default::default()
    };
    plenty::compile_file_to_executable_with_options(&source, &executable, None, &options).unwrap();
    let output = success(Command::new(&executable).output().unwrap());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "8 [\"a\", \"bc\"]\n"
    );
    let header = success(
        Command::new("readelf")
            .arg("-hdW")
            .arg(&executable)
            .output()
            .unwrap(),
    );
    let header = String::from_utf8(header.stdout).unwrap();
    assert!(
        header.contains("DYN ("),
        "not position-independent:\n{header}"
    );
    assert!(!header.contains("TEXTREL"), "text relocations:\n{header}");
}
