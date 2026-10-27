//! `if const` selects declarations and statements by target facts while
//! parsing; only the selected branch is resolved, checked and compiled.
use std::process::Command;

fn run(source: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.plenty");
    std::fs::write(&main, source).unwrap();
    let executable = dir.path().join("program");
    plenty::compile_file_to_executable(&main, &executable, None).unwrap();
    let output = Command::new(executable).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

// The supported target is x86_64 Linux (`toolchain::validate_target`); these
// expectations change when another target is added.
#[test]
fn module_scope_branches_select_declarations() {
    let output = run("\
if const target.os == \"windows\":
    import windows_only
    def platform() -> str:
        return windows_only.name()
elif const target.os == \"linux\" and target.arch == \"x86_64\":
    def platform() -> str:
        return \"linux-x86_64\"
    const WORD_BITS: i64 = 64
else:
    def platform() -> str:
        return undefined_name
    const WORD_BITS: i64 = 32

if const target.os == \"macos\":
    pass

if const not (target.pointer_width == 32) and target.endian != \"big\":
    assert WORD_BITS == 64

def main() -> Result[(), IoError]:
    print(platform())?
    print(WORD_BITS)?
    Ok(())
");
    assert_eq!(output, "linux-x86_64\n64\n");
}

#[test]
fn function_body_branches_splice_into_the_enclosing_block() {
    let output = run("\
def word_bytes() -> i64:
    if const target.pointer_width == 32:
        4
    else:
        8

def main() -> Result[(), IoError]:
    if const target.os == \"linux\":
        name = \"linux\"
    elif const target.os == \"macos\":
        name = 7
        print(missing)?
    print(name)?
    if const target.arch == \"aarch64\":
        print(\"arm\")?
    print(word_bytes())?
    Ok(())
");
    assert_eq!(output, "linux\n8\n");
}

#[test]
fn target_conditions_are_checked() {
    for (condition, message) in [
        ("target.cpu == \"x\"", "unknown target fact `target.cpu`"),
        ("target.os == \"linx\"", "unknown value for `target.os`"),
        (
            "target.pointer_width == 16",
            "unknown value for `target.pointer_width`",
        ),
        (
            "target.os == target.arch",
            "compares one `target` fact with a literal",
        ),
        ("1 == 1", "compares one `target` fact with a literal"),
        ("WIDTH == 64", "compares one `target` fact with a literal"),
        ("target.os", "compares `target` facts with `==` or `!=`"),
        ("target.os == 1 + 1", "needs a string or integer literal"),
    ] {
        for source in [
            format!(
                "const WIDTH: i64 = 64\nif const {condition}:\n    pass\ndef main() -> ():\n    pass\n"
            ),
            format!(
                "const WIDTH: i64 = 64\ndef main() -> ():\n    if const {condition}:\n        pass\n"
            ),
        ] {
            let error = plenty::check_source(&source).unwrap_err().to_string();
            assert!(error.contains(message), "{source}: {error}");
        }
    }
}

#[test]
fn unselected_branches_must_still_parse() {
    for source in [
        "if const target.os == \"windows\":\n    def broken(:\n        pass\ndef main() -> ():\n    pass\n",
        "def main() -> ():\n    if const target.os == \"windows\":\n        x = (\n",
        "if const target.os == \"linux\":\n    pass\nelif target.os == \"macos\":\n    pass\ndef main() -> ():\n    pass\n",
        "def main() -> ():\n    if const target.os == \"linux\":\n        pass\n    elif target.os == \"macos\":\n        pass\n",
    ] {
        assert!(plenty::check_source(source).is_err(), "{source}");
    }
    let error = plenty::check_source(
        "if const target.os == \"linux\":\n    pass\nelif target.os == \"macos\":\n    pass\ndef main() -> ():\n    pass\n",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("`elif const`"), "{error}");
}
