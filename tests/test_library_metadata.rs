use plenty::{LibraryKind, LibraryOptions};
use std::process::Command;

fn contract(name: &str) -> String {
    format!("# plenty-interface-format: 1\n# library: {name}\n# target: {}\n# abi: C\n\npub extern def answer() -> i32 = \"{name}_answer\"\n", plenty::native_target())
}

fn object(sections: &[(&str, &[u8])]) -> Vec<u8> {
    let mut object = object::write::Object::new(
        object::BinaryFormat::Elf,
        object::Architecture::X86_64,
        object::Endianness::Little,
    );
    for (name, data) in sections {
        let section = object.add_section(
            Vec::new(),
            name.as_bytes().to_vec(),
            object::SectionKind::ReadOnlyData,
        );
        object.append_section_data(section, data, 1);
    }
    object.write().unwrap()
}

#[test]
fn extracts_exact_contract_from_static_shared_and_stripped_artifacts() {
    for kind in [LibraryKind::Static, LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.plenty");
        std::fs::write(
            &source,
            "export def answer() -> i32 = \"calc_answer\":\n    42\n",
        )
        .unwrap();
        let library = temp.path().join("library");
        let artifacts = plenty::compile_file_to_library(
            &source,
            &library,
            None,
            &LibraryOptions::new("calc", kind),
        )
        .unwrap();
        if kind == LibraryKind::Shared {
            assert!(Command::new("strip")
                .arg("--strip-all")
                .arg(&library)
                .status()
                .unwrap()
                .success());
        }
        let expected = std::fs::read_to_string(artifacts.interface).unwrap();
        let interfaces = plenty::read_library_interfaces(&library).unwrap();
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].name, "calc");
        assert_eq!(interfaces[0].source, expected);
        let output = temp.path().join("extracted.plentyi");
        let result = Command::new(env!("CARGO_BIN_EXE_plenty"))
            .arg("--extract-interface")
            .arg(&library)
            .args(["--library-name", "calc", "-o"])
            .arg(&output)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(std::fs::read_to_string(&output).unwrap(), expected);
        assert!(plenty::extract_library_interface(&library, "missing", &output).is_err());
        assert_eq!(std::fs::read_to_string(&output).unwrap(), expected);
        assert!(plenty::extract_library_interface(&library, "calc", &library).is_err());
    }
}

#[test]
fn rejects_malformed_ambiguous_and_oversized_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("input.o");
    let valid = contract("calc");
    let cases = [
        (object(&[(".rodata", valid.as_bytes())]), "no embedded"),
        (
            object(&[(".plenty.interface.wrong", valid.as_bytes())]),
            "disagrees",
        ),
        (
            object(&[(
                ".plenty.interface.calc",
                valid.replace("format: 1", "format: 9").as_bytes(),
            )]),
            "unsupported",
        ),
        (object(&[(".plenty.interface.calc", b"\xff")]), "utf-8"),
        (object(&[(".plenty.interface.calc", b"\0")]), "NUL"),
        (
            object(&[(
                ".plenty.interface.calc",
                valid.replace("# abi: C", "# abi: rust").as_bytes(),
            )]),
            "ABI",
        ),
        (
            object(&[
                (".plenty.interface.calc", valid.as_bytes()),
                (".plenty.interface.calc", valid.as_bytes()),
            ]),
            "duplicate",
        ),
        (
            object(&[(".plenty.interface.calc", &vec![b' '; 4 * 1024 * 1024 + 1])]),
            "limit",
        ),
        (b"not an object".to_vec(), "Could not"),
    ];
    for (data, expected) in cases {
        std::fs::write(&path, data).unwrap();
        let error = plenty::read_library_interfaces(&path)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{expected}: {error}");
    }
    let second = contract("other");
    std::fs::write(
        &path,
        object(&[
            (".plenty.interface.other", second.as_bytes()),
            (".plenty.interface.calc", valid.as_bytes()),
        ]),
    )
    .unwrap();
    assert_eq!(
        plenty::read_library_interfaces(&path)
            .unwrap()
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
        ["calc", "other"]
    );
    let thin = temp.path().join("thin.a");
    assert!(Command::new("ar")
        .arg("crsT")
        .arg(&thin)
        .arg(&path)
        .status()
        .unwrap()
        .success());
    assert!(plenty::read_library_interfaces(&thin)
        .unwrap_err()
        .to_string()
        .contains("thin"));
}
