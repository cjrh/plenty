//! Nominal package identities in compiler graphs, independently of resolution.
use plenty::project::{PackageProvenance, SourceGraph, SourcePackage};
use std::path::Path;
use std::process::{Command, Output};

fn write(path: &Path, source: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn package(root: &Path, directory: &str, name: &str, version: &str) -> SourcePackage {
    let full = format!("fixture:{directory}");
    SourcePackage {
        identity: SourcePackage::resolved_identity(name, version, &full),
        source_identity: full,
        provenance: Some(PackageProvenance {
            name: name.into(),
            version: version.into(),
            source: format!("local fixture {directory}"),
            reached: format!("app -> {directory}"),
        }),
        test_of: None,
        directory: Some(root.join(directory)),
        root: root.join(directory).join("src"),
        library: Some(root.join(directory).join("src/lib.plenty")),
        dependencies: vec![],
    }
}

fn graph_fixture(root: &Path) -> SourceGraph {
    let mut app = package(root, "app", "app", "1.0.0");
    app.library = None;
    app.dependencies = vec![("old".into(), 1), ("new".into(), 2)];
    SourceGraph {
        packages: vec![
            app,
            package(root, "old", "cells", "1.0.0"),
            package(root, "new", "cells", "2.0.0"),
        ],
    }
}

fn compile(root: &Path, graph: &SourceGraph, check: bool) -> Output {
    write(&root.join("graph.toml"), &graph.to_toml());
    let mut command = Command::new(env!("CARGO_BIN_EXE_plentyc"));
    command
        .current_dir(root)
        .args(["--package-graph", "graph.toml"]);
    if check {
        command.arg("--check");
    }
    command.arg("app/src/main.plenty").output().unwrap()
}

const CELL: &str = "pub class Cell[T]:\n    pub value: T\n    pub def read(self) -> T:\n        self.value\n\npub protocol Read:\n    def read(self) -> i64:\n        pass\n";

#[test]
fn distinct_versions_keep_generic_symbols_and_structural_protocols() {
    let dir = tempfile::tempdir().unwrap();
    let graph = graph_fixture(dir.path());
    for version in ["old", "new"] {
        write(&dir.path().join(version).join("src/lib.plenty"), CELL);
    }
    write(
        &dir.path().join("app/src/main.plenty"),
        r#"import old
import new
protocol Readable:
    def read(self) -> i64:
        pass
def read[T: Readable](value: &T) -> i64:
    value.read()
def main() -> Result[(), IoError]:
    first = old.Cell(11)
    second = new.Cell(22)
    print(read(&first), read(&second))?
    Ok(())
"#,
    );
    let output = compile(dir.path(), &graph, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"11 22\n");
    let roundtrip = SourceGraph::read(&dir.path().join("graph.toml")).unwrap();
    assert_eq!(graph.to_toml(), roundtrip.to_toml());
}

#[test]
fn nominal_and_boxed_protocol_mismatches_show_both_origins() {
    let dir = tempfile::tempdir().unwrap();
    let graph = graph_fixture(dir.path());
    for version in ["old", "new"] {
        write(&dir.path().join(version).join("src/lib.plenty"), CELL);
    }
    for (main, expected) in [
        (
            "def take(value: old.Cell[i64]) -> ():\n    pass\ndef main() -> ():\n    take(new.Cell(22))\n",
            "expected",
        ),
        (
            "def take(value: Box[old.Read]) -> ():\n    pass\ndef main() -> Result[(), AllocError]:\n    value = Box[new.Read](new.Cell(22))?\n    take(value)\n    Ok(())\n",
            "expected Box[",
        ),
    ] {
        write(
            &dir.path().join("app/src/main.plenty"),
            &format!("import old\nimport new\n{main}"),
        );
        let output = compile(dir.path(), &graph, true);
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success(),
            "unexpected successful compilation"
        );
        for text in [
            expected,
            "cells 1.0.0",
            "cells 2.0.0",
            "app -> old",
            "app -> new",
            "fixture:old",
            "fixture:new",
        ] {
            assert!(error.contains(text), "missing {text}: {error}");
        }
    }
}

#[test]
fn graph_identity_validation_rejects_tampering_and_unknown_versions() {
    let dir = tempfile::tempdir().unwrap();
    let graph = graph_fixture(dir.path());
    let path = dir.path().join("graph.toml");
    write(
        &path,
        &graph.to_toml().replace("fixture:new", "fixture:other"),
    );
    assert!(
        SourceGraph::read(&path)
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    write(
        &path,
        &graph
            .to_toml()
            .replace("graph-version = 2", "graph-version = 99"),
    );
    assert!(
        SourceGraph::read(&path)
            .unwrap_err()
            .to_string()
            .contains("unsupported graph-version")
    );
    let mut collision = graph.clone();
    collision.packages[2].identity = collision.packages[1].identity.clone();
    write(&path, &collision.to_toml());
    assert!(
        SourceGraph::read(&path)
            .unwrap_err()
            .to_string()
            .contains("collision")
    );
}

#[test]
fn synthetic_test_ownership_survives_graph_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = graph_fixture(dir.path());
    let owner = graph.packages[1].clone();
    let mut tests = owner.clone();
    tests.source_identity = format!("tests:{}", owner.source_identity);
    tests.provenance.as_mut().unwrap().name = "cells_tests".into();
    tests.identity =
        SourcePackage::resolved_identity("cells_tests", "1.0.0", &tests.source_identity);
    tests.root = owner.directory.as_ref().unwrap().join("tests");
    tests.library = None;
    tests.test_of = Some(owner.identity.clone());
    graph.packages.push(tests);
    let path = dir.path().join("graph.toml");
    write(&path, &graph.to_toml());
    let read = SourceGraph::read(&path).unwrap();
    assert_eq!(read.packages[3].test_of.as_ref(), Some(&owner.identity));
    let mut forged = graph.clone();
    forged.packages[3].provenance = None;
    forged.packages[3].source_identity.push_str("modified");
    write(&path, &forged.to_toml());
    assert!(
        SourceGraph::read(&path)
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    graph.packages[3].test_of = Some("unknown".into());
    write(&path, &graph.to_toml());
    assert!(
        SourceGraph::read(&path)
            .unwrap_err()
            .to_string()
            .contains("unknown owner")
    );
}

#[test]
fn equal_versions_from_distinct_sources_remain_nominally_distinct_in_the_ide() {
    let dir = tempfile::tempdir().unwrap();
    let mut graph = graph_fixture(dir.path());
    let new = &mut graph.packages[2];
    new.provenance.as_mut().unwrap().version = "1.0.0".into();
    new.identity = SourcePackage::resolved_identity("cells", "1.0.0", &new.source_identity);
    for version in ["old", "new"] {
        write(&dir.path().join(version).join("src/lib.plenty"), CELL);
    }
    let main = dir.path().join("app/src/main.plenty");
    write(
        &main,
        "import old\nimport new\ndef take(value: old.Cell[i64]) -> ():\n    pass\ndef main() -> ():\n    take(new.Cell(22))\n",
    );
    let analysis = plenty::ide::analyze(&main, Some(&graph), Default::default());
    let diagnostic = analysis
        .diagnostics
        .iter()
        .find(|d| d.message.contains("expected"))
        .unwrap();
    assert_eq!(diagnostic.line, 6, "{diagnostic:?}");
    assert!(diagnostic.file.is_some(), "{diagnostic:?}");
    for origin in ["fixture:old", "fixture:new", "app -> old", "app -> new"] {
        assert!(diagnostic.message.contains(origin), "{diagnostic:?}");
    }
}

#[test]
fn structural_protocol_results_require_the_exact_nominal_type() {
    let dir = tempfile::tempdir().unwrap();
    let graph = graph_fixture(dir.path());
    for version in ["old", "new"] {
        write(
            &dir.path().join(version).join("src/lib.plenty"),
            &format!(
                "{CELL}\npub class Maker:\n    pub def get(self) -> Cell[i64]:\n        Cell(1)\n"
            ),
        );
    }
    write(
        &dir.path().join("app/src/main.plenty"),
        "import old\nimport new\nprotocol Make:\n    def get(self) -> old.Cell[i64]:\n        pass\ndef take[T: Make](value: &T) -> ():\n    pass\ndef main() -> ():\n    value = new.Maker()\n    take(&value)\n",
    );
    let output = compile(dir.path(), &graph, true);
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    for expected in ["signature", "cells 1.0.0", "cells 2.0.0"] {
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn a_shared_identity_preserves_nominal_types_through_multiple_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut app = package(root, "app", "app", "1.0.0");
    app.library = None;
    app.dependencies = vec![("a".into(), 1), ("b".into(), 2)];
    let mut a = package(root, "a", "a", "1.0.0");
    a.dependencies = vec![("first".into(), 3)];
    let mut b = package(root, "b", "b", "1.0.0");
    b.dependencies = vec![("second".into(), 3)];
    let graph = SourceGraph {
        packages: vec![app, a, b, package(root, "old", "cells", "1.0.0")],
    };
    write(&root.join("old/src/lib.plenty"), CELL);
    write(
        &root.join("a/src/lib.plenty"),
        "import first\npub def make() -> first.Cell[i64]:\n    first.Cell(31)\n",
    );
    write(
        &root.join("b/src/lib.plenty"),
        "import second\npub def take(value: second.Cell[i64]) -> i64:\n    value.value\n",
    );
    write(
        &root.join("app/src/main.plenty"),
        "import a\nimport b\ndef main() -> Result[(), IoError]:\n    print(b.take(a.make()))?\n    Ok(())\n",
    );
    let output = compile(root, &graph, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"31\n");
}
