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

#[test]
fn generation_rejects_unusable_names_without_replacing_previous_output() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let producer = root.join("source.plenty");
    let output = root.join("plugin.plentyi");
    std::fs::write(&output, "previous").unwrap();
    for name in ["new", "self", "_private"] {
        std::fs::write(
            &producer,
            format!("export def {name}() -> i32 = \"calc_value\":\n    1\n"),
        )
        .unwrap();
        let error = plenty::emit_runtime_interface(&producer, &output, None, "calc")
            .unwrap_err()
            .to_string();
        assert!(error.contains("reserved"), "{error}");
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "previous");
    }
    for name in ["Library", "load"] {
        std::fs::write(&producer, format!("class {name}:\n    value: i32\nexport def create() -> Result[{name}, AllocError] = \"calc_create\":\n    Ok({name}(1))\n")).unwrap();
        let error = plenty::emit_runtime_interface(&producer, &output, None, "calc")
            .unwrap_err()
            .to_string();
        assert!(error.contains("runtime interface API"), "{error}");
        assert_eq!(std::fs::read_to_string(&output).unwrap(), "previous");
    }
    std::fs::write(
        &producer,
        format!(
            "export def value() -> i32 = \"calc_{}\":\n    1\n",
            "x".repeat(512)
        ),
    )
    .unwrap();
    let error = plenty::emit_runtime_interface(&producer, &output, None, "calc")
        .unwrap_err()
        .to_string();
    assert!(error.contains("512 bytes"), "{error}");
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "previous");
}

#[test]
fn loaded_wrappers_keep_raw_addresses_private_and_enforce_moves() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let producer = root.join("source.plenty");
    std::fs::write(&producer, "class Resource:\n    value: i32\nexport def create() -> Result[Resource, AllocError] = \"calc_create\":\n    Ok(Resource(1))\nexport def read(value: &Resource) -> i32 = \"calc_read\":\n    value.value\nexport def finish(value: Resource) -> () = \"calc_finish\":\n    drop(value)\n").unwrap();
    plenty::emit_runtime_interface(&producer, &root.join("plugin.plentyi"), None, "calc").unwrap();
    let entry = root.join("main.plenty");
    for (body, expected) in [
        ("    address = library._origin\n", "private"),
        ("    owner = library.create()?\n    library.finish(owner)\n    print(library.read(&owner))?\n", "moved"),
        ("    owner = library.create()?\n    raw = owner._handle\n", "private"),
    ] {
        std::fs::write(&entry, format!("import plugin\ndef main() -> Result[(), Failure]:\n    path = \"./libcalc.so\"\n    library = plugin.load(&path)?\n{body}    Ok(())\n")).unwrap();
        let error = plenty::check_file(&entry, None).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}
