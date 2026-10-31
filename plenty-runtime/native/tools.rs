//! Resolve tools for the Cargo target, never silently use host cc in a cross build.
use std::ffi::{OsStr, OsString};
use std::process::Command;

pub fn tool(kind: &str) -> OsString {
    resolve(
        kind,
        &std::env::var("TARGET").expect("Cargo TARGET"),
        &std::env::var("HOST").expect("Cargo HOST"),
        |key| {
            println!("cargo:rerun-if-env-changed={key}");
            std::env::var_os(key)
        },
    )
}

/// A deterministic Unix-style archive command for Cargo's target. Callers
/// append the output archive and object paths, after removing any old archive.
/// This does not provide the MSVC librarian interface.
pub fn archive_command() -> Command {
    archive_command_for(&std::env::var("TARGET").expect("Cargo TARGET"), &tool("AR"))
}

fn archive_command_for(target: &str, ar: &OsStr) -> Command {
    let mut command = Command::new(ar);
    if target.contains("-apple-") {
        // Apple's ar lacks GNU's D flag; ZERO_AR_DATE zeros member and index
        // timestamps instead. See apple-oss-distributions/cctools ar/archive.c
        // and libstuff/writeout.c, also used by rust-lang/cc-rs.
        command.arg("crs").env("ZERO_AR_DATE", "1");
    } else {
        command.arg("crsD");
    }
    command
}

fn resolve(
    kind: &str,
    target: &str,
    host: &str,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> OsString {
    for key in [
        format!("{kind}_{target}"),
        format!("{kind}_{}", target.replace('-', "_")),
        format!("TARGET_{kind}"),
        kind.to_owned(),
    ] {
        if let Some(value) = lookup(&key) {
            return value;
        }
    }
    if target == host {
        return if kind == "CC" { "cc" } else { "ar" }.into();
    }
    let prefix = match target {
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        _ => panic!("set {kind}_{target} explicitly for this cross target"),
    };
    format!("{prefix}-{}", if kind == "CC" { "gcc" } else { "ar" }).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_policy_uses_the_target_not_the_build_host() {
        for target in ["aarch64-apple-darwin", "x86_64-apple-darwin"] {
            let command = archive_command_for(target, OsStr::new("chosen-ar"));
            assert_eq!(command.get_program(), "chosen-ar");
            assert_eq!(command.get_args().collect::<Vec<_>>(), ["crs"]);
            assert!(
                command
                    .get_envs()
                    .any(|(key, value)| key == "ZERO_AR_DATE" && value == Some(OsStr::new("1")))
            );
        }
        for target in ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"] {
            let command = archive_command_for(target, OsStr::new("cross-ar"));
            assert_eq!(command.get_program(), "cross-ar");
            assert_eq!(command.get_args().collect::<Vec<_>>(), ["crsD"]);
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn native_archive_is_linkable_and_ignores_object_mtime() {
        use std::fs::{File, FileTimes};
        use std::time::{Duration, SystemTime};

        struct Scratch(std::path::PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = Scratch(std::env::temp_dir().join(format!(
            "plenty-archive-test-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
        )));
        std::fs::create_dir(&root.0).unwrap();
        let source = root.0.join("answer.c");
        let object = root.0.join("answer.o");
        let archive = root.0.join("libanswer.a");
        let main = root.0.join("main.c");
        let executable = root.0.join("program");
        std::fs::write(&source, "int answer(void) { return 42; }\n").unwrap();
        std::fs::write(
            &main,
            "extern int answer(void); int main(void) { return answer() == 42 ? 0 : 1; }\n",
        )
        .unwrap();
        let checked = |command: &mut Command| {
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "command failed: {}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        };
        checked(
            Command::new("cc")
                .arg("-c")
                .arg(&source)
                .arg("-o")
                .arg(&object),
        );
        let arch = std::env::consts::ARCH;
        let target = if cfg!(target_vendor = "apple") {
            format!("{arch}-apple-darwin")
        } else {
            format!("{arch}-unknown-linux-gnu")
        };
        let mut snapshots = Vec::new();
        for seconds in [1_000_000_000, 1_500_000_000] {
            let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
            File::open(&object)
                .unwrap()
                .set_times(FileTimes::new().set_modified(time))
                .unwrap();
            assert_eq!(
                std::fs::metadata(&object).unwrap().modified().unwrap(),
                time
            );
            let _ = std::fs::remove_file(&archive);
            checked(
                archive_command_for(&target, OsStr::new("ar"))
                    .arg(&archive)
                    .arg(&object),
            );
            snapshots.push(std::fs::read(&archive).unwrap());
        }
        assert_eq!(
            snapshots[0], snapshots[1],
            "object timestamps leaked into archive"
        );
        // Linking and running requires a usable archive symbol index, not
        // merely a valid archive header or a successful archiver exit.
        checked(
            Command::new("cc")
                .arg(&main)
                .arg(&archive)
                .arg("-o")
                .arg(&executable),
        );
        checked(&mut Command::new(&executable));
    }

    #[test]
    fn target_override_precedes_generic_and_host_tools() {
        let found = resolve(
            "CC",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            |key| match key {
                "CC_aarch64_unknown_linux_gnu" => Some("cross-gcc".into()),
                "CC" => Some("host-cc".into()),
                _ => None,
            },
        );
        assert_eq!(found, "cross-gcc");
    }
    #[test]
    fn cross_default_never_uses_host_cc_or_ar() {
        for (kind, expected) in [
            ("CC", "aarch64-linux-gnu-gcc"),
            ("AR", "aarch64-linux-gnu-ar"),
        ] {
            assert_eq!(
                resolve(
                    kind,
                    "aarch64-unknown-linux-gnu",
                    "x86_64-unknown-linux-gnu",
                    |_| None
                ),
                expected
            );
        }
        assert_eq!(
            resolve(
                "CC",
                "aarch64-unknown-linux-gnu",
                "aarch64-unknown-linux-gnu",
                |_| None
            ),
            "cc"
        );
    }
}
