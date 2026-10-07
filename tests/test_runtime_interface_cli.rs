use std::process::Command;

#[test]
fn command_generates_without_a_toolchain_and_preserves_source_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source = root.join("producer.plenty");
    let helper = root.join("helper.plentyi");
    std::fs::write(&helper, "pub def offset() -> i32:\n    2\n").unwrap();
    std::fs::write(&source, "from helper import offset\nexport def add(value: i32) -> i32 = \"calc_add\":\n    value + offset()\n").unwrap();
    let output = root.join("plugin.plentyi");
    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_plenty"));
        command
            .arg("--runtime-interface")
            .arg(&source)
            .args(["--library-name", "calc", "--module-root"])
            .arg(root);
        command
    };
    let result = command()
        .arg("-o")
        .arg(&output)
        .env("PATH", "/nonexistent/plenty-test")
        .output()
        .unwrap();
    assert!(result.status.success(), "{result:?}");
    plenty::check_module_file(&output, None).unwrap();
    assert_eq!(
        std::fs::read_to_string(&output).unwrap(),
        plenty::runtime_interface_source(&source, Some(root), "calc").unwrap()
    );
    let original = std::fs::read(&helper).unwrap();
    let result = command().arg("-o").arg(&helper).output().unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("source"),
        "{result:?}"
    );
    assert_eq!(std::fs::read(&helper).unwrap(), original);
    let link = root.join("link.plentyi");
    std::os::unix::fs::symlink(&helper, &link).unwrap();
    assert!(!command()
        .arg("-o")
        .arg(&link)
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(std::fs::read(&helper).unwrap(), original);
    let old = std::fs::read(&output).unwrap();
    for flags in [
        vec!["--link-arg", "bad"],
        vec!["--archiver", "bad"],
        vec!["--target", "aarch64-unknown-linux-gnu"],
    ] {
        let result = command()
            .arg("-o")
            .arg(&output)
            .args(flags)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(std::fs::read(&output).unwrap(), old);
    }
    let wrong_extension = root.join("plugin.plenty");
    assert!(!command()
        .arg("-o")
        .arg(&wrong_extension)
        .output()
        .unwrap()
        .status
        .success());
    assert!(!wrong_extension.exists());
}
