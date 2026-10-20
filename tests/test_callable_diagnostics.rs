mod support;
use support::{check_source, run};

#[test]
fn code_only_parameters_explain_closure_state_and_show_generic_borrowing() {
    for (parameters, call) in [
        ("callback: Callable[[i64], i64]", "apply(add, 4)"),
        ("callback: &Callable[[i64], i64]", "apply(&add, 4)"),
        (
            "callback: Callable[[i64], i64], extra: T",
            "apply(add, 4, 1)",
        ),
    ] {
        let generic = if parameters.contains("extra") {
            "[T]"
        } else {
            ""
        };
        let source = format!("def apply{generic}({parameters}, value: i64) -> i64:\n    callback(value)\ndef main() -> ():\n    offset = 3\n    add = def [offset](value: i64) -> i64:\n        value + offset\n    {call}\n");
        // The generic form puts `extra` before `value`; both are i64 here.
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains("stores only a code address"), "{error}");
        assert!(error.contains("F: Callable[[i64], i64]"), "{error}");
        assert!(error.contains("callback: &F"), "{error}");
        assert!(error.contains("defined at 5:11"), "{error}");
        assert!(!error.contains("__plenty_closure_"), "{error}");
    }
}

#[test]
fn mutable_and_consuming_callbacks_get_their_own_ownership_advice() {
    for (closure, expected_bound, expected_parameter) in [
        (
            "def [mut offset]",
            "F: Callable[[i64], i64]",
            "callback: &mut F",
        ),
        (
            "def once [offset]",
            "F: OnceCallable[[i64], i64]",
            "callback: F",
        ),
    ] {
        let source = format!("def apply(callback: Callable[[i64], i64], value: i64) -> i64:\n    callback(value)\ndef main() -> ():\n    offset = 3\n    add = {closure}(value: i64) -> i64:\n        value + offset\n    apply(add, 4)\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(error.contains(expected_bound), "{error}");
        assert!(error.contains(expected_parameter), "{error}");
    }
}

#[test]
fn different_signatures_do_not_suggest_that_a_generic_bound_will_fix_them() {
    let error = check_source("def apply(callback: Callable[[i64], i64]) -> ():\n    pass\ndef main() -> ():\n    offset = 3u8\n    add = def [offset](value: u8) -> u8:\n        value + offset\n    apply(add)\n").unwrap_err().to_string();
    assert!(error.contains("call signatures also differ"), "{error}");
    assert!(!error.contains("for a generic callback API"), "{error}");

    let error = check_source("def apply[F: Callable[[i64], i64]](callback: &F) -> ():\n    pass\ndef main() -> ():\n    offset = 3u8\n    add = def [offset](value: u8) -> u8:\n        value + offset\n    apply(&add)\n").unwrap_err().to_string();
    assert!(
        error.contains("does not satisfy Callable[[i64], i64]"),
        "{error}"
    );
    assert!(
        error.contains("Closure[[u8], u8] defined at 5:11"),
        "{error}"
    );
    assert!(!error.contains("__plenty_closure_"), "{error}");
}

#[test]
fn one_shot_closures_explain_why_the_reusable_bound_is_not_satisfied() {
    let error = check_source("def apply[F: Callable[[], i64]](callback: F) -> ():\n    pass\ndef main() -> ():\n    callback = def once () -> i64:\n        1\n    apply(callback)\n").unwrap_err().to_string();
    assert!(
        error.contains("OnceClosure[[], i64] defined at 4:16"),
        "{error}"
    );
    assert!(
        error.contains("one-shot closure is consumed by its call"),
        "{error}"
    );
    assert!(error.contains("F: OnceCallable[[], i64]"), "{error}");
    assert!(error.contains("owned `F` parameter"), "{error}");
}

#[test]
fn borrowed_one_shot_callbacks_do_not_get_impossible_transfer_advice() {
    for parameter in ["&Callable[[], i64]", "&F"] {
        let bound = if parameter == "&F" {
            "[F: Callable[[], i64]]"
        } else {
            ""
        };
        let source = format!("def ignore{bound}(callback: {parameter}) -> ():\n    pass\ndef main() -> ():\n    offset = 3\n    callback = def once [&offset]() -> i64:\n        offset\n    ignore(&callback)\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains("borrowed captures can only be consumed locally"),
            "{error}"
        );
        assert!(!error.contains("owned `F` parameter"), "{error}");
        assert!(!error.contains("callback: F"), "{error}");
    }
}

#[test]
fn distinct_closure_expressions_report_both_origins() {
    let error = check_source("def main() -> ():\n    offset = 3\n    mut callback = def [offset]() -> i64:\n        offset\n    other = def [offset]() -> i64:\n        offset\n    callback = other\n").unwrap_err().to_string();
    assert!(
        error.contains("Closure[[], i64] defined at 3:20"),
        "{error}"
    );
    assert!(
        error.contains("Closure[[], i64] defined at 5:13"),
        "{error}"
    );
    assert!(!error.contains("__plenty_closure_"), "{error}");
}

#[test]
fn source_locations_do_not_unify_different_generic_environments() {
    for wrapper in [
        "{}",
        "Some({})",
        "({}, 0)",
        "Holder({})",
        "[{}].unwrap()",
        "wrap({})",
    ] {
        let original = wrapper.replace("{}", "make(1u8)");
        let replacement = wrapper.replace("{}", "make(2u16)");
        let source = format!("def make[T](value: T) -> Closure[[], i64]:\n    def [value]() -> i64:\n        1\nclass Holder[T]:\n    value: T\ndef wrap[T](value: T) -> Result[T, AllocError]:\n    Ok(value)\ndef main() -> ():\n    mut callback = {original}\n    callback = {replacement}\n");
        let error = check_source(&source).unwrap_err().to_string();
        assert_eq!(
            error.matches("Closure[[], i64] defined at 2:5").count(),
            2,
            "{wrapper}: {error}"
        );
        assert!(!error.contains("__plenty_closure_"), "{error}");
    }
}

#[test]
fn stored_generic_environments_keep_their_own_specialized_code() {
    let output = run("class Holder[F: Callable[[], i64]]:\n    callback: F\n    def call(self) -> i64:\n        self.callback()\ndef make[T: IntType](value: T) -> Closure[[], i64]:\n    def [value]() -> i64:\n        i64(value)\ndef main() -> ():\n    first = Holder(make(7u8))\n    second = Holder(make(300u16))\n    print(first.call()).unwrap()\n    print(second.call()).unwrap()\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "7\n300\n");
}

#[test]
fn imported_closures_report_the_definition_file() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("callbacks.plenty");
    let main = dir.path().join("main.plenty");
    std::fs::write(&library, "pub def make(offset: i64) -> Closure[[i64], i64]:\n    def [offset](value: i64) -> i64:\n        value + offset\n").unwrap();
    std::fs::write(&main, "import callbacks\ndef apply(callback: Callable[[i64], i64]) -> ():\n    pass\ndef main() -> ():\n    apply(callbacks.make(3))\n").unwrap();
    let error = plenty::check_file(&main, None).unwrap_err().to_string();
    assert!(
        error.contains(&format!("defined at {}:2:5", library.display())),
        "{error}"
    );
    assert!(error.contains("F: Callable[[i64], i64]"), "{error}");
    assert!(!error.contains("__plenty_closure_"), "{error}");
}

#[test]
fn unresolved_closure_returns_name_closure_identity_not_generators() {
    // Issue #140: an error-only body leaves the success closure's environment unknown.
    for (annotation, kind) in [
        ("OnceClosure[[i64], i64]", "one-shot closure"),
        ("Closure[[i64], i64]", "closure"),
    ] {
        let source = format!(
            "def mapper() -> Result[{annotation}, str]:\n    Err(\"no\")\ndef main() -> ():\n    drop(mapper())\n"
        );
        let error = check_source(&source).unwrap_err().to_string();
        assert!(
            error.contains(&format!("cannot infer the concrete {kind} returned")),
            "{error}"
        );
        assert!(error.contains("captured environment"), "{error}");
        assert!(!error.contains("generator"), "{error}");
    }
    let generator = check_source(
        "def numbers() -> Result[Generator[i64], str]:\n    Err(\"no\")\ndef main() -> ():\n    drop(numbers())\n",
    )
    .unwrap_err()
    .to_string();
    assert!(
        generator.contains("cannot infer the concrete generator returned"),
        "{generator}"
    );
}
