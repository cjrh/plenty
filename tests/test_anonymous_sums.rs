//! Anonymous sums preserve concrete values through normalization and widening.
use std::process::Command;

fn runs(source: &str, expected: &str) {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("main.plenty");
    let binary = workspace.path().join("program");
    std::fs::write(&file, source).unwrap();
    plenty::compile_file_to_executable(&file, &binary, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    let output = Command::new(binary).output().unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let visible: String = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(visible, expected, "{source}");
}

fn rejects(source: &str, alternatives: &[&str]) {
    let error = plenty::check_source(source).unwrap_err().to_string();
    assert!(
        alternatives.iter().any(|text| error.contains(text)),
        "{source}\nexpected one of {alternatives:?}, got {error}"
    );
}

#[test]
fn aliases_order_duplicates_and_nested_sums_have_one_identity() {
    runs(
        r#"
enum A:
    Value(i64)
enum B:
    Value(i64)
type Alias = A
type First = A | B
type Second = B | (Alias | A)
def read(value: Second) -> i64:
    match value:
        case a: A:
            match a:
                case A.Value(n):
                    n
        case b: B:
            match b:
                case B.Value(n):
                    n
def main() -> ():
    a: First = A.Value(11)
    b: Second = B.Value(22)
    print(read(a)).unwrap()
    print(read(b)).unwrap()
    one: Alias | A = A.Value(33)
    match one:
        case a: A:
            match a:
                case A.Value(n):
                    print(n).unwrap()
"#,
        "11\n22\n33\n",
    );
}

#[test]
fn injection_covers_bindings_assignments_arguments_and_return_branches() {
    runs(
        r#"
enum A:
    Value
enum B:
    Value
def choose(first: bool) -> A | B:
    if first:
        return A.Value
    B.Value
def read(value: A | B) -> i64:
    match value:
        case a: A:
            1
        case b: B:
            2
def main() -> ():
    mut value: A | B = A.Value
    print(read(value)).unwrap()
    value = B.Value
    print(read(value)).unwrap()
    print(read(A.Value)).unwrap()
    print(read(choose(True))).unwrap()
    print(read(choose(False))).unwrap()
"#,
        "1\n2\n1\n1\n2\n",
    );
}

#[test]
fn named_enum_members_keep_their_own_variants() {
    runs(
        r#"
enum Inner:
    Left(i64)
    Right(i64)
enum Other:
    Value
def read(value: Inner | Other) -> i64:
    match value:
        case inner: Inner:
            match inner:
                case Inner.Left(n):
                    n
                case Inner.Right(n):
                    n + 1
        case other: Other:
            0
def main() -> ():
    print(read(Inner.Left(4))).unwrap()
    print(read(Inner.Right(4))).unwrap()
"#,
        "4\n5\n",
    );
}

#[test]
fn result_widening_moves_success_and_retags_each_error_member() {
    runs(
        r#"
enum A:
    Value(i64)
enum B:
    Value(i64)
enum C:
    Value
class Resource:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
def widen(value: Result[Resource, B | A]) -> Result[Resource, C | A | B]:
    value
def inspect(value: Result[Resource, A | B | C]) -> ():
    match value:
        case Ok(resource):
            print("success").unwrap()
            drop(resource)
        case Err(error):
            match error:
                case a: A:
                    match a:
                        case A.Value(n):
                            print(n).unwrap()
                case b: B:
                    match b:
                        case B.Value(n):
                            print(n).unwrap()
                case c: C:
                    print("unexpected").unwrap()
def main() -> ():
    success: Result[Resource, A | B] = Ok(Resource(10))
    first: Result[Resource, A | B] = Err(A.Value(20))
    second: Result[Resource, A | B] = Err(B.Value(30))
    inspect(widen(success))
    inspect(widen(first))
    inspect(widen(second))
"#,
        "success\n10\n20\n30\n",
    );
}

#[test]
fn widening_preserves_nested_result_and_enum_payload_tags() {
    runs(
        r#"
enum A:
    Left(i64)
    Right(i64)
enum B:
    Value
enum C:
    Value
def widen(value: Result[i64, Result[i64, A] | B]) -> Result[i64, C | B | Result[i64, A]]:
    value
def inspect(value: Result[i64, C | B | Result[i64, A]]) -> ():
    match value:
        case Ok(number):
            print(number).unwrap()
        case Err(error):
            match error:
                case inner: Result[i64, A]:
                    match inner:
                        case Ok(number):
                            print(number).unwrap()
                        case Err(a):
                            match a:
                                case A.Left(n):
                                    print(n).unwrap()
                                case A.Right(n):
                                    print(n + 1).unwrap()
                case b: B:
                    print("B").unwrap()
                case c: C:
                    print("C").unwrap()
def main() -> ():
    nested_ok: Result[i64, A] = Ok(10)
    first: Result[i64, Result[i64, A] | B] = Err(nested_ok)
    nested_error: Result[i64, A] = Err(A.Right(20))
    second: Result[i64, Result[i64, A] | B] = Err(nested_error)
    inspect(widen(first))
    inspect(widen(second))
    only: A | A = A.Right(30)
    match only:
        case a: A:
            match a:
                case A.Left(n):
                    print(n).unwrap()
                case A.Right(n):
                    print(n + 1).unwrap()
"#,
        "10\n21\n31\n",
    );
}

#[test]
fn question_mark_widens_error_and_handles_unit_success() {
    runs(
        r#"
enum A:
    Value(i64)
enum B:
    Value
def source(fail: bool) -> Result[(), A]:
    if fail:
        return Err(A.Value(7))
    Ok(())
def propagated(fail: bool) -> Result[i64, A | B]:
    source(fail)?
    Ok(42)
def main() -> ():
    match propagated(False):
        case Ok(value):
            print(value).unwrap()
        case Err(_):
            print("unexpected").unwrap()
    match propagated(True):
        case Ok(_):
            print("unexpected").unwrap()
        case Err(error):
            match error:
                case a: A:
                    match a:
                        case A.Value(n):
                            print(n).unwrap()
                case b: B:
                    print("unexpected").unwrap()
"#,
        "42\n7\n",
    );
}

#[test]
fn borrowing_member_payloads_preserves_and_mutates_the_original() {
    runs(
        r#"
class Counter:
    pub value: i64
enum Empty:
    Value
def read(value: &(Counter | Empty)) -> i64:
    match value:
        case counter: Counter:
            counter.value
        case empty: Empty:
            0
def change(value: &mut (Counter | Empty)) -> ():
    match value:
        case counter: Counter:
            counter.value = 41
        case empty: Empty:
            pass
def main() -> ():
    mut value: Counter | Empty = Counter(7)
    print(read(&value)).unwrap()
    change(&mut value)
    print(read(&value)).unwrap()
    drop(value)
"#,
        "7\n41\n",
    );
}

#[test]
fn producers_run_once_and_only_active_owned_payloads_are_destroyed() {
    runs(
        r#"
class First:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
class Second:
    id: i64
    def __del__(self) -> ():
        print(self.id).unwrap()
enum Third:
    Value
def make() -> First:
    print("produce").unwrap()
    First(10)
def widen(value: First | Second) -> First | Second | Third:
    value
def discard(value: First | Second | Third) -> ():
    match value:
        case _:
            print("discard").unwrap()
def fail(value: First | Second) -> Result[(), First | Second | Third]:
    Err(value)
def early() -> Result[(), First | Second | Third]:
    local = Second(30)
    fail(First(40))?
    drop(local)
    Ok(())
def main() -> ():
    narrow: First | Second = make()
    discard(widen(narrow))
    discard(Second(20))
    drop(early())
"#,
        "produce\ndiscard\n10\ndiscard\n20\n30\n40\n",
    );
}

#[test]
fn generic_substitution_can_collapse_to_one_member() {
    runs(
        r#"
enum A:
    Value(i64)
def identity[T](value: T | T) -> T:
    match value:
        case member: T:
            member
def main() -> ():
    match identity[A](A.Value(19)):
        case A.Value(n):
            print(n).unwrap()
"#,
        "19\n",
    );
}

#[test]
fn unique_generic_members_infer_and_other_arguments_supply_membership_evidence() {
    runs(
        r#"
def choose[T](value: T | bool) -> i64:
    match value:
        case member: T:
            1
        case flag: bool:
            2
def known[T](first: T, second: T | bool) -> i64:
    drop(first)
    match second:
        case member: T:
            3
        case flag: bool:
            4
def main() -> ():
    print(choose(42)).unwrap()
    print(known(42, True)).unwrap()
    print(known(42, 43)).unwrap()
"#,
        "1\n4\n3\n",
    );
}

#[test]
fn generic_membership_does_not_guess_ambiguous_substitutions() {
    rejects(
        "def choose[T, U](value: T | U) -> ():\n    drop(value)\ndef main() -> ():\n    choose(42)\n",
        &["infer", "ambiguous", "explicit"],
    );
}

#[test]
fn callable_brackets_and_grouped_sums_do_not_change_tuple_boundaries() {
    runs(
        r#"
enum A:
    Value
enum B:
    Value
def choose(value: A | B) -> (B | A):
    value
def invoke(function: Callable[[(A | B)], A | B], value: A | B) -> A | B:
    function(value)
def main() -> ():
    function: Callable[[A | B], (B | A)] = choose
    tuple: (A,) = (A.Value,)
    match invoke(function, tuple[0]):
        case a: A:
            print("A").unwrap()
        case b: B:
            print("B").unwrap()
"#,
        "A\n",
    );
}

#[test]
fn unique_members_supply_context_for_constructors_and_tuples() {
    runs(
        r#"
enum A:
    Value
def option(value: Option[i64] | str) -> i64:
    match value:
        case optional: Option[i64]:
            match optional:
                case Some(n):
                    n
                case Nothing:
                    0
        case text: str:
            -1
def result(value: Result[i64, A] | str) -> i64:
    match value:
        case outcome: Result[i64, A]:
            match outcome:
                case Ok(n):
                    n
                case Err(_):
                    -2
        case text: str:
            -1
def pair(value: (i64, i64) | str) -> i64:
    match value:
        case numbers: (i64, i64):
            numbers[0] + numbers[1]
        case text: str:
            -1
def main() -> ():
    present: Option[i64] | str = Some(7)
    absent: Option[i64] | str = Nothing
    success: Result[i64, A] | str = Ok(8)
    failed: Result[i64, A] | str = Err(A.Value)
    tuple: (i64, i64) | str = (9, 10)
    print(option(present)).unwrap()
    print(option(absent)).unwrap()
    print(result(success)).unwrap()
    print(result(failed)).unwrap()
    print(pair(tuple)).unwrap()
    print(option(Some(11))).unwrap()
    print(result(Ok(12))).unwrap()
    print(pair((13, 14))).unwrap()
"#,
        "7\n0\n8\n-2\n19\n11\n12\n27\n",
    );
}

#[test]
fn unit_values_inject_and_match_without_becoming_empty_sums() {
    runs(
        r#"
def read(value: () | i64) -> i64:
    match value:
        case unit: ():
            0
        case number: i64:
            number
def main() -> ():
    unit: () | i64 = ()
    print(read(unit)).unwrap()
    print(read(())).unwrap()
    print(read(17)).unwrap()
"#,
        "0\n0\n17\n",
    );
}

#[test]
fn matching_requires_every_member_and_rejects_duplicate_aliases() {
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\ndef read(value: A | B) -> ():\n    match value:\n        case a: A:\n            pass\ndef main() -> ():\n    read(A.Value)\n",
        &["exhaustive", "missing"],
    );
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\ntype Alias = A\ndef read(value: A | B) -> ():\n    match value:\n        case a: A:\n            pass\n        case again: Alias:\n            pass\n        case b: B:\n            pass\ndef main() -> ():\n    read(A.Value)\n",
        &["duplicate", "unreachable"],
    );
}

#[test]
fn duplicate_generic_arms_are_checked_after_specialization() {
    rejects(
        "enum A:\n    Value\ndef read[T](value: T | A) -> ():\n    match value:\n        case generic: T:\n            pass\n        case fixed: A:\n            pass\ndef main() -> ():\n    read[A](A.Value)\n",
        &["duplicate", "unreachable"],
    );
}

#[test]
fn missing_widening_alternatives_are_named() {
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\nenum C:\n    Value\ndef narrow(value: A | B) -> A | C:\n    value\ndef main() -> ():\n    drop(narrow(B.Value))\n",
        &["B"],
    );
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\nenum C:\n    Value\ndef source() -> Result[i64, A | B]:\n    Err(B.Value)\ndef narrow() -> Result[i64, A | C]:\n    Ok(source()?)\ndef main() -> ():\n    drop(narrow())\n",
        &["B"],
    );
}

#[test]
fn widening_does_not_change_success_types_or_make_containers_covariant() {
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\ndef widen(value: Result[i64, A]) -> Result[bool, A | B]:\n    value\ndef main() -> ():\n    drop(widen(Ok(1)))\n",
        &["expected", "cannot", "incompatible"],
    );
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\ndef widen(value: list[A]) -> list[A | B]:\n    value\ndef main() -> ():\n    drop(widen([A.Value].unwrap()))\n",
        &["expected", "cannot", "incompatible"],
    );
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\ndef widen(value: &A) -> &(A | B):\n    value\ndef main() -> ():\n    value = A.Value\n    drop(widen(&value))\n",
        &["expected", "cannot", "incompatible"],
    );
}

#[test]
fn shared_member_borrows_cannot_mutate_or_outlive_the_owner() {
    rejects(
        "class Counter:\n    pub value: i64\nenum Empty:\n    Value\ndef change(value: &(Counter | Empty)) -> ():\n    match value:\n        case counter: Counter:\n            counter.value = 1\n        case empty: Empty:\n            pass\ndef main() -> ():\n    value: Counter | Empty = Counter(0)\n    change(&value)\n",
        &["mutable", "shared", "borrow"],
    );
    rejects(
        "class Counter:\n    pub value: i64\nenum Empty:\n    Value\ndef main() -> ():\n    value: Counter | Empty = Counter(0)\n    match &value:\n        case counter: Counter:\n            drop(value)\n            print(counter.value).unwrap()\n        case empty: Empty:\n            pass\n",
        &["borrow", "moved"],
    );
}

#[test]
fn guards_and_nested_member_patterns_have_explicit_rejections() {
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\ndef read(value: A | B) -> ():\n    match value:\n        case a: A if True:\n            pass\n        case _:\n            pass\ndef main() -> ():\n    read(A.Value)\n",
        &["guard", "expected `:`"],
    );
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\ndef read(value: Option[A | B]) -> ():\n    match value:\n        case Some(a: A):\n            pass\n        case _:\n            pass\ndef main() -> ():\n    read(Some(A.Value))\n",
        &["top-level", "top level", "expected `,`", "member"],
    );
}

#[test]
fn singleton_normalization_preserves_unit_and_reference_conventions() {
    runs(
        r#"
def done() -> () | ():
    ()
def same(value: &i64 | &i64) -> &i64:
    value
def main() -> ():
    done()
    n = 5
    print(*same(&n)).unwrap()
"#,
        "5\n",
    );
}

#[test]
fn generic_result_inference_preserves_lossless_error_widening() {
    runs(
        r#"
enum A:
    Value
enum B:
    Value
def keep[T](value: Result[T, A | B]) -> Result[T, A | B]:
    value
def main() -> ():
    narrow: Result[i64, A] = Ok(7)
    print(keep(narrow).unwrap()).unwrap()
    failed: Result[i64, A] = Err(A.Value)
    print(keep(failed).is_err()).unwrap()
"#,
        "7\nTrue\n",
    );
    rejects(
        "enum A:\n    Value\nenum B:\n    Value\ndef keep[T](n: T, value: list[A | B]) -> T:\n    n\ndef main() -> ():\n    narrow = [A.Value].unwrap()\n    drop(keep(7, narrow))\n",
        &["expected"],
    );
}

#[test]
fn collection_results_widen_without_losing_literal_context() {
    runs(
        r#"
def values() -> Result[list[i64], AllocError | ParseError]:
    [1, 2]
def main() -> ():
    print(values().unwrap()).unwrap()
    mixed: Result[list[i64], AllocError] | str = [3]
    match mixed:
        case result: Result[list[i64], AllocError]:
            print(result.unwrap()).unwrap()
        case text: str:
            print(text).unwrap()
"#,
        "[1, 2]\n[3]\n",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn injection_widening_and_matching_need_no_runtime_allocation() {
    runs(
        r#"
enum A:
    Value(i64)
enum B:
    Value
enum C:
    Value
def widen(value: Result[list[i64], A | B]) -> Result[list[i64], A | B | C]:
    value
def read(value: A | B | C) -> i64:
    match value:
        case a: A:
            match a:
                case A.Value(n):
                    n
        case b: B:
            0
        case c: C:
            0
def main() -> Result[(), AllocError | IoError]:
    success: Result[list[i64], A | B] = Ok([3, 4]?)
    print("__test_fail_allocations_after_0__")?
    print("__test_begin_no_allocations__")?
    narrow: A | B = A.Value(9)
    wide: A | B | C = narrow
    number = read(wide)
    kept = widen(success)
    failed: Result[list[i64], A | B] = Err(B.Value)
    drop(widen(failed))
    print("__test_end_no_allocations__")?
    print("__test_restore_allocations__")?
    print(number)?
    print(kept.unwrap())?
    Ok(())
"#,
        "9\n[3, 4]\n",
    );
}
