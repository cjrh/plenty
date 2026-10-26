//! `std.fs.dirs`: the home and XDG base directories, read from an environment
//! the test sets exactly.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROGRAM: &str = r#"import std.fs.dirs
from std.path import Path

def show(label: &str, dir: Option[Path]) -> Result[(), IoError]:
    match dir:
        case Some(path):
            print(label, path.display()?)?
        case Nothing:
            print(label, "-")?
    Ok(())

def main() -> Result[(), IoError]:
    show("home", std.fs.dirs.home_dir()?)?
    show("config", std.fs.dirs.config_dir()?)?
    show("cache", std.fs.dirs.cache_dir()?)?
    show("data", std.fs.dirs.data_dir()?)?
    show("state", std.fs.dirs.state_dir()?)?
    Ok(())
"#;

fn compile(dir: &Path) -> PathBuf {
    let source = dir.join("main.plenty");
    let executable = dir.join("program");
    std::fs::write(&source, PROGRAM).unwrap();
    plenty::compile_file_to_executable(&source, &executable, None)
        .unwrap_or_else(|error| panic!("{error}"));
    executable
}

/// Runs with only `variables` set.
fn run(executable: &Path, variables: &[(&str, &[u8])]) -> String {
    let mut command = Command::new(executable);
    command.env_clear();
    for (name, value) in variables {
        command.env(name, OsStr::from_bytes(value));
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn directories_follow_xdg_rules() {
    let dir = tempfile::tempdir().unwrap();
    let program = compile(dir.path());

    assert_eq!(
        run(&program, &[("HOME", b"/home/ada")]),
        "home /home/ada\nconfig /home/ada/.config\ncache /home/ada/.cache\n\
         data /home/ada/.local/share\nstate /home/ada/.local/state\n",
        "defaults below HOME"
    );

    assert_eq!(
        run(
            &program,
            &[
                ("HOME", b"/home/ada"),
                ("XDG_CONFIG_HOME", b"/cfg"),
                ("XDG_CACHE_HOME", b"/var/cache/ada"),
                ("XDG_DATA_HOME", b"/data"),
                ("XDG_STATE_HOME", b"/state"),
            ]
        ),
        "home /home/ada\nconfig /cfg\ncache /var/cache/ada\ndata /data\nstate /state\n",
        "absolute XDG values win"
    );

    assert_eq!(
        run(
            &program,
            &[
                ("HOME", b"/home/ada"),
                ("XDG_CONFIG_HOME", b"relative"),
                ("XDG_CACHE_HOME", b""),
                ("XDG_DATA_HOME", b"./data"),
            ]
        ),
        "home /home/ada\nconfig /home/ada/.config\ncache /home/ada/.cache\n\
         data /home/ada/.local/share\nstate /home/ada/.local/state\n",
        "relative and empty XDG values are ignored"
    );

    assert_eq!(
        run(&program, &[("XDG_CACHE_HOME", b"/c")]),
        "home -\nconfig -\ncache /c\ndata -\nstate -\n",
        "no HOME: only absolute XDG values"
    );

    assert_eq!(
        run(&program, &[("HOME", b"")]),
        "home -\nconfig -\ncache -\ndata -\nstate -\n",
        "empty HOME is unset"
    );

    assert_eq!(
        run(
            &program,
            &[("HOME", b"/h\xff"), ("XDG_STATE_HOME", b"/s\xfe")]
        ),
        "home /h\\xff\nconfig /h\\xff/.config\ncache /h\\xff/.cache\n\
         data /h\\xff/.local/share\nstate /s\\xfe\n",
        "native bytes are preserved"
    );
}
