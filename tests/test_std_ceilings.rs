//! Std fixed ceilings reject statically known inputs without changing the
//! underlying domain type or removing the dynamic runtime guard.
mod support;

fn error(imports: &str, body: &str) -> String {
    support::check_file_error(&format!("{imports}\ndef main() -> ():\n    {body}\n"))
}

#[test]
fn fixed_parameter_ceilings_reject_literals() {
    for (imports, body, parameter, ceiling) in [
        (
            "import std.crypto",
            "drop(std.crypto.hkdf_sha256(b\"\", b\"\", b\"\", 9000))",
            "size",
            8160_i64,
        ),
        (
            "import std.sketch",
            "drop(std.sketch.quantiles(1048577))",
            "capacity",
            1048576,
        ),
        (
            "import std.sketch",
            "drop(std.sketch.frequent_items(65537))",
            "capacity",
            65536,
        ),
        (
            "import std.sketch.xor",
            "drop(std.sketch.xor.from_bytes(b\"\", 16777217))",
            "max_keys",
            16777216,
        ),
        (
            "import std.sketch.cuckoo",
            "drop(std.sketch.cuckoo.from_bytes(b\"\", 16777217))",
            "max_buckets",
            16777216,
        ),
        (
            "import std.fmt",
            "drop(std.fmt.string_formatter(8, 65))",
            "max_depth",
            64,
        ),
        (
            "import std.fmt",
            "drop(std.fmt.to_string(&1, 10, 65))",
            "max_depth",
            64,
        ),
        (
            "import std.unicode",
            "drop(std.unicode.nfd(\"\", 2305843009213693952))",
            "max_scalars",
            2305843009213693951,
        ),
        (
            "import std.jwt",
            "drop(std.jwt.decode_header(\"\", 1048577))",
            "max_size",
            1048576,
        ),
        (
            "import std.image",
            "drop(std.image.new(2147483648, 1, std.image.ColorType.Gray))",
            "width",
            2147483647,
        ),
        (
            "import std.tempfile",
            "drop(std.tempfile.directory(max_depth=65))",
            "max_depth",
            64,
        ),
    ] {
        let diagnostic = error(imports, body);
        assert!(
            diagnostic.contains(&format!("constant argument `{parameter}`")),
            "{body}: {diagnostic}"
        );
        assert!(
            diagnostic.contains(&format!("above its fixed ceiling of {ceiling}")),
            "{body}: {diagnostic}"
        );
    }
}

#[test]
fn named_defaults_aliases_and_domain_constants_keep_the_ceiling() {
    let source = r#"
import std.crypto as crypto
const too_large: NonNegative[i64] = 8000 + 161

def main() -> ():
    drop(crypto.hkdf_sha256(info=b"", size=too_large, salt=b"", key_material=b""))
"#;
    let diagnostic = support::check_file_error(source);
    assert!(
        diagnostic.contains("constant argument `size` is 8161"),
        "{diagnostic}"
    );
    let diagnostic = error(
        "from std.sketch import quantiles",
        "drop(quantiles(seed=7u64, capacity=(0x10_0001)))",
    );
    assert!(
        diagnostic.contains("constant argument `capacity` is 1048577"),
        "{diagnostic}"
    );
}

#[test]
fn class_construction_and_field_updates_check_the_declared_ceiling() {
    for body in [
        "drop(std.json.Limits(1025, 10, 10, std.json.DuplicateKeys.Reject))",
        "drop(std.json.Limits(max_input=10, max_depth=1025, max_output=10, duplicate_keys=std.json.DuplicateKeys.Reject))",
        "mut limits = std.json.limits()\n    limits.max_depth = 1025",
        "mut limits = std.json.limits()\n    drop(replace(limits.max_depth, 1025))",
    ] {
        let diagnostic = error("import std.json", body);
        assert!(
            diagnostic.contains("constant argument `max_depth` is 1025"),
            "{body}: {diagnostic}"
        );
    }
    let diagnostic = support::check_file_error(
        "import std.json\ndef change(limits: &mut std.json.Limits) -> ():\n    limits.max_depth = 1025\ndef main() -> ():\n    pass\n",
    );
    assert!(
        diagnostic.contains("constant argument `max_depth` is 1025"),
        "{diagnostic}"
    );
}

#[test]
fn fs_receiver_methods_check_ceilings_before_lowering_arguments() {
    for source in [
        "import std.fs\nimport std.path\ndef remove(root: &std.fs.Root, name: &std.path.Path, cancel: &CancellationToken) -> ():\n    drop(std.fs.remove_tree(root, name, 1, 65, cancel))\ndef main() -> ():\n    pass\n",
        "import std.fs\ndef cleanup(dir: &mut std.fs.TempDir, cancel: &CancellationToken) -> ():\n    drop(dir.cleanup_tree(1, 65, cancel))\ndef main() -> ():\n    pass\n",
    ] {
        let diagnostic = support::check_file_error(source);
        assert!(
            diagnostic.contains("constant argument `max_depth` is 65"),
            "{diagnostic}"
        );
    }
}

#[test]
fn std_ceiling_marker_cannot_be_written_by_user_code() {
    for source in [
        "def f(value: Positive[i64] __max(10)) -> ():\n    pass\n",
        "class Limits:\n    pub depth: NonNegative[i64] __max(10)\n",
    ] {
        let diagnostic = support::check_file_error(source);
        assert!(
            diagnostic.contains("`__max` is reserved for standard-library"),
            "{diagnostic}"
        );
    }
}

#[test]
fn exact_boundaries_remain_valid_and_dynamic_values_keep_their_domain_type() {
    let source = r#"
import std.crypto
import std.json
import std.sketch

def derive(size: NonNegative[i64]) -> ():
    drop(std.crypto.hkdf_sha256(b"", b"", b"", size))

def main() -> ():
    drop(std.crypto.hkdf_sha256(b"", b"", b"", 0))
    drop(std.crypto.hkdf_sha256(b"", b"", b"", 8160))
    drop(std.sketch.quantiles())
    drop(std.sketch.quantiles(capacity=1048576))
    mut limits = std.json.Limits(1024, 0, 0, std.json.DuplicateKeys.Reject)
    limits.max_depth = 1024
    derive(9000)
"#;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    plenty::check_file(&path, None).unwrap();
}

#[test]
fn plain_module_constants_and_constant_operators_are_checked() {
    let source = "import std.sketch\nconst limit: Positive[i64] = 1048577\ndef main() -> ():\n    drop(std.sketch.quantiles(limit))\n";
    let diagnostic = support::check_file_error(source);
    assert!(
        diagnostic.contains("constant argument `capacity` is 1048577"),
        "{diagnostic}"
    );
    let diagnostic = error(
        "import std.sketch",
        "drop(std.sketch.quantiles(1048576 + 1))",
    );
    assert!(
        diagnostic.contains("constant argument `capacity` is 1048577"),
        "{diagnostic}"
    );
}

#[test]
fn numeric_module_constants_and_domain_getters_are_checked() {
    for argument in ["too_large", "domain_limit.get()", "domain_limit.get() + 1"] {
        let source = format!(
            "import std.crypto\nconst too_large: i64 = 8161\nconst domain_limit: NonNegative[i64] = 8161\ndef main() -> ():\n    drop(std.crypto.hkdf_sha256(b\"\", b\"\", b\"\", {argument}))\n"
        );
        let diagnostic = support::check_file_error(&source);
        assert!(
            diagnostic.contains("constant argument `size`"),
            "{argument}: {diagnostic}"
        );
        assert!(
            diagnostic.contains("above its fixed ceiling of 8160"),
            "{argument}: {diagnostic}"
        );
    }
}

#[test]
fn domain_preserving_constant_arithmetic_is_checked() {
    let source = "import std.sketch\nconst capacity: Positive[i64] = 600000\ndef main() -> ():\n    drop(std.sketch.quantiles(capacity * 2))\n";
    let diagnostic = support::check_file_error(source);
    assert!(
        diagnostic.contains("constant argument `capacity` is 1200000"),
        "{diagnostic}"
    );
    let source = "import std.crypto\nconst size: NonNegative[i64] = 4000\ndef main() -> ():\n    drop(std.crypto.hkdf_sha256(b\"\", b\"\", b\"\", size + size + 161))\n";
    let diagnostic = support::check_file_error(source);
    assert!(
        diagnostic.contains("constant argument `size` is 8161"),
        "{diagnostic}"
    );
}
