//! An installed compiler must only link the already-built Rust runtime.
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
fn script(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn relocated_compiler_uses_only_objects_and_a_packaged_archive() {
    let temp = tempfile::tempdir().unwrap();
    let paths = std::env::var_os("PATH").unwrap();
    let cc = std::env::split_paths(&paths)
        .map(|p| p.join("cc"))
        .find(|p| p.is_file())
        .expect("system linker driver");
    let log = temp.path().join("link-args");
    script(
        &temp.path().join("cc"),
        &format!(
            r#"#!/bin/sh
archive=0
for arg in "$@"; do
    case "$arg" in
        *.c|*.rs) echo 'unexpected runtime compilation' >&2; exit 90 ;;
        *libplenty_runtime.a) archive=1 ;;
    esac
done
[ "$archive" = 1 ] || exit 91
printf '%s\n' "$@" >> {}
exec {} "$@"
"#,
            quote(&log),
            quote(&cc)
        ),
    );
    for tool in ["rustc", "cargo"] {
        script(
            &temp.path().join(tool),
            "#!/bin/sh\necho 'Rust toolchain must not be invoked' >&2\nexit 92\n",
        );
    }
    let installed = temp.path().join("plenty");
    std::fs::copy(env!("CARGO_BIN_EXE_plenty"), &installed).unwrap();
    let paths = std::env::join_paths(
        std::iter::once(temp.path().to_path_buf()).chain(std::env::split_paths(&paths)),
    )
    .unwrap();
    let source = temp.path().join("program.plenty");
    std::fs::write(
        &source,
        r#"
class Resource:
    name: str
    def __del__(self) -> ():
        print(self.name).unwrap()
def values() -> Generator[str]:
    yield "é\0🦀"

def main() -> ():
    print(list(values().unwrap()).unwrap()).unwrap()
    drop(Resource("done").unwrap())
"#,
    )
    .unwrap();
    let executable = temp.path().join("program");
    let result = Command::new(installed)
        .current_dir(temp.path())
        .env("PATH", paths)
        .arg("--compile")
        .arg(source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let arguments = std::fs::read_to_string(log).unwrap();
    assert_eq!(
        arguments
            .lines()
            .filter(|s| s.ends_with("libplenty_runtime.a"))
            .count(),
        1
    );
    let result = Command::new(executable).env_clear().output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&result.stdout),
        "[\"é\\0🦀\"]\ndone\n"
    );
}
