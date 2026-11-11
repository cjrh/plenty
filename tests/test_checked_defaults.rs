//! Function arithmetic contracts and lexical trapping opt-outs (B14).
mod support;
use support::{check_diagnostic, run_stdout};

#[test]
fn functions_use_the_declared_result_without_wrapping_successes() {
    assert_eq!(
        run_stdout(
            r#"
def scale(a: i64, b: i64) -> Result[i64, OverflowError[i64]]:
    Ok(a * b)
def early(a: i64) -> Result[i64, OverflowError[i64] | ParseError]:
    if a == 0:
        return Ok(42)
    product = a * 2
    return Ok(product)
def caller(a: i64) -> Result[i64, OverflowError[i64]]:
    value = unchecked:
        scale(a, 2)?
    Ok(value)
def main() -> Result[(), IoError]:
    print(scale(3, 4).unwrap(), scale(i64.MAX, 2).is_err())?
    print(early(0).unwrap(), early(i64.MAX).is_err())?
    print(caller(i64.MAX).is_err())?
    Ok(())
"#
        ),
        "12 True\n42 True\nTrue\n"
    );
}

#[test]
fn errors_are_required_at_the_operation_not_inferred_from_callers() {
    for (body, error) in [
        ("a + 1", "OverflowError[i64]"),
        ("a // b", "ZeroDivisionError"),
        ("a % b", "ZeroDivisionError"),
    ] {
        let source = format!(
            "def bad(a: i64, b: i64) -> i64:\n    {body}\ndef main() -> ():\n    drop(bad(1, 2))\n"
        );
        let diagnostic = check_diagnostic(&source);
        assert!(diagnostic.contains(error), "{diagnostic}");
        assert!(diagnostic.contains("checked:"), "{diagnostic}");
        assert!(diagnostic.contains("unchecked:"), "{diagnostic}");
    }
    let diagnostic = check_diagnostic(
        "def bad(a: i64) -> Option[i64]:\n    Some(a + 1)\ndef main() -> ():\n    drop(bad(0))\n",
    );
    assert!(diagnostic.contains("OverflowError[i64]"), "{diagnostic}");
    let diagnostic = check_diagnostic(
        "def bad(a: i64) -> Result[i64, OverflowError[i64]]:\n    a + 1\ndef main() -> ():\n    drop(bad(0))\n",
    );
    assert!(diagnostic.contains("expected Result"), "{diagnostic}");
}

#[test]
fn divisor_proofs_remove_errors_without_creating_an_unbound_failure_edge() {
    assert_eq!(
        run_stdout(
            r#"
def quotient(a: i64, b: Positive[i64]) -> i64:
    a // b
def remainder(a: i64, b: NonZero[i64]) -> i64:
    a % b
def unsigned(a: u64, b: NonZero[u64]) -> u64:
    a // b
def signed(a: i64, b: NonZero[i64]) -> Result[i64, OverflowError[i64]]:
    Ok(a // b)
def main() -> Result[(), IoError | RangeError]:
    positive = Positive.try_from(3)?
    nonzero = NonZero.try_from(-1)?
    other = NonZero.try_from(2u64)?
    print(quotient(14, positive), remainder(i64.MIN, nonzero), unsigned(7u64, other))?
    print(signed(i64.MIN, nonzero).is_err())?
    Ok(())
"#
        ),
        "4 0 3\nTrue\n"
    );
}

#[test]
fn local_recovery_and_nesting_restore_the_outer_mode() {
    assert_eq!(
        run_stdout(
            r#"
def recovered(a: i64) -> i64:
    result = checked:
        a + 1
    match result:
        case Ok(value):
            value
        case Err(_):
            17
def mixed(a: i64) -> Result[i64, OverflowError[i64]]:
    result = unchecked:
        offset = a + 1
        inner = checked:
            offset * 2
        inner
    result
def restored(a: i64) -> Result[i64, OverflowError[i64]]:
    unchecked:
        pass
    Ok(a + 1)
def main() -> Result[(), IoError]:
    print(recovered(2), recovered(i64.MAX))?
    print(mixed(2).unwrap(), mixed(4611686018427387903).is_err())?
    print(restored(i64.MAX).is_err())?
    Ok(())
"#
        ),
        "3 17\n6 True\nTrue\n"
    );
}

#[test]
fn unchecked_supports_value_and_statement_positions_and_contextual_names() {
    assert_eq!(
        run_stdout(
            r#"
def value(a: i64) -> i64:
    return unchecked:
        a * 2
def tail(a: i64) -> i64:
    unchecked:
        unchecked:
            a + 1
def arm(a: i64) -> i64:
    match a:
        case _:
            unchecked:
                a + 2
def main() -> Result[(), IoError]:
    unchecked: i64 = 7
    mut x = 0
    x = unchecked:
        unchecked + 1
    mut n = 0
    while n < 4:
        unchecked:
            n += 1
            if n == 2:
                continue
            if n == 3:
                break
            x += n
    print(value(3), tail(3), arm(3), x, n)?
    Ok(())
"#
        ),
        "6 4 5 9 3\n"
    );
    let diagnostic = check_diagnostic(
        "def main() -> ():\n    unchecked:\n        value: Result[i64, ParseError] = Ok(1)\n        value\n    pass\n",
    );
    assert!(
        diagnostic.contains("cannot implicitly discard a Result"),
        "{diagnostic}"
    );
}

#[test]
fn closure_contracts_are_independent_of_definition_and_call_modes() {
    assert_eq!(
        run_stdout(
            r#"
def make() -> Callable[[i64], Result[i64, OverflowError[i64]]]:
    unchecked:
        (def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + 1))
def main() -> Result[(), IoError]:
    f = make()
    print(f(3).unwrap(), f(i64.MAX).is_err())?
    closure = unchecked:
        offset = 2
        (def [offset](x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + offset))
    print(closure(3).unwrap(), closure(i64.MAX).is_err())?
    result = checked:
        local = (def(x: i64) -> Result[i64, OverflowError[i64]]: Ok(x + 1))
        print(local(i64.MAX).is_err())?
        len("a") * 3
    print(result.unwrap())?
    Ok(())
"#
        ),
        "4 True\n5 True\nTrue\n3\n"
    );
    for body in [
        "unchecked:\n        (def(x: i64) -> i64: x + 1)",
        "checked:\n        (def(x: i64) -> i64: x + 1)",
    ] {
        let source = format!(
            "def make() -> Callable[[i64], i64]:\n    {body}\ndef main() -> ():\n    f = make()\n    drop(f(0))\n"
        );
        let diagnostic = check_diagnostic(&source);
        assert!(
            diagnostic.contains("OverflowError[i64]")
                || diagnostic.contains("contains no arithmetic"),
            "{diagnostic}"
        );
    }
    let diagnostic = check_diagnostic(
        r#"
def main() -> Result[(), AllocError]:
    xs = [1, 2]?
    drop(iter(&xs).map(def(x: &i64) -> i64: *x + 1).count())
    Ok(())
"#,
    );
    assert!(diagnostic.contains("OverflowError[i64]"), "{diagnostic}");
}

#[test]
fn implicit_failures_release_locals_and_pending_temporaries_without_writing() {
    assert_eq!(
        run_stdout(
            r#"
class Owner:
    id: i64
    def __del__(self: &mut Owner) -> ():
        print(self.id).unwrap()
def fail(n: &mut i64) -> Result[(), AllocError | OverflowError[i64]]:
    owner = Owner(10)
    items = [1, 2, 3]?
    *n += i64.MAX
    drop(owner)
    drop(items)
    Ok(())
def main() -> Result[(), IoError]:
    mut n = 1
    print(fail(&mut n).is_err(), n)?
    Ok(())
"#
        ),
        "10\nTrue 1\n"
    );
}

#[test]
fn generators_require_local_handling_or_explicit_traps() {
    let diagnostic = check_diagnostic(
        r#"
def values(n: i64) -> Generator[i64]:
    yield n + 1
def main() -> ():
    drop(values(0))
"#,
    );
    assert!(
        diagnostic.contains("generator cannot propagate arithmetic errors"),
        "{diagnostic}"
    );
    assert_eq!(
        run_stdout(
            r#"
def values(n: i64) -> Generator[i64]:
    unchecked:
        yield n + 1
def main() -> Result[(), IoError]:
    for value in values(3):
        print(value)?
    Ok(())
"#
        ),
        "4\n"
    );
}

#[test]
fn function_arithmetic_keeps_native_recursive_tail_returns() {
    assert_eq!(
        run_stdout(
            r#"
def count(n: i64) -> Result[i64, OverflowError[i64]]:
    if n == 0:
        return Ok(n)
    return count(n - 1)
def main() -> Result[(), IoError]:
    print(count(100000).unwrap())?
    Ok(())
"#
        ),
        "0\n"
    );
}

#[test]
fn integer_reductions_have_public_results_in_either_lexical_mode() {
    assert_eq!(
        run_stdout(
            r#"
import std.math as math

def main() -> Result[(), AllocError | IoError]:
    values = [i64.MAX, 1]?
    result = unchecked:
        iter(&values).sum()
    print(result.is_err(), math.sum(&values, 0).is_err())?
    match result:
        case Ok(_):
            pass
        case Err(error):
            print(error.wrapped)?
    print(math.abs(i64.MIN).is_err())?
    product = [i8.MAX, 2i8]?
    match math.prod(&product, 1i8):
        case Ok(_):
            pass
        case Err(error):
            print(error.wrapped)?
    finite = [1.0f32, 2.0f32]?
    print(math.sum_float(&finite, 0.0f32), iter(&finite).sum())?
    print(math.abs_float(-0.0f32), math.prod_float(&finite, 1.0f32))?
    big = [1.7e308, 1.7e308]?
    print(math.sum_float(&big, 0.0), iter(&big).sum())?
    empty: list[i64] = []?
    print(iter(&empty).sum().unwrap(), math.sum(&empty, 9).unwrap())?
    print(len(values), len(product))?
    Ok(())
"#
        ),
        "True True\n-9223372036854775808\nTrue\n-2\n3.0 3.0\n0.0 2.0\ninf inf\n0 9\n2 2\n"
    );
}

#[test]
fn public_callback_failures_release_captures_locals_and_pending_arguments_once() {
    let output = run_stdout(
        r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def add(a: Resource, b: i64) -> Result[i64, OverflowError[i64]]:
    Ok(a.id + b)
def apply[F: Callable[[i64], Result[i64, OverflowError[i64]]]](f: &F, x: i64) -> Result[i64, OverflowError[i64]]:
    local = Resource(20)
    add(Resource(30), f(x)?)
def main() -> Result[(), IoError]:
    resource = Resource(10)
    f = def [resource](x: i64) -> Result[i64, OverflowError[i64]]:
        local = Resource(40)
        Ok(resource.id + x)
    result = apply(&f, i64.MAX)
    drop(f)
    print(result.is_err())?
    Ok(())
"#,
    );
    for id in [10, 20, 30, 40] {
        assert_eq!(
            output
                .lines()
                .filter(|line| *line == format!("drop {id}"))
                .count(),
            1,
            "{output}"
        );
    }
    assert!(output.ends_with("True\n"), "{output}");
}

#[test]
fn fallible_key_sort_uses_public_results_and_preserves_affine_rows() {
    let output = run_stdout(
        r#"
class Row:
    id: i64
    name: str
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def main() -> Result[(), AllocError | IoError]:
    mut rows = [Row(0, "a".repeat(1000)?), Row(1, "b".repeat(1000)?), Row(2, "c".repeat(1000)?)]?
    key = def(row: &Row) -> Result[(str, i64), OverflowError[i64]]:
        print("key", row.id).unwrap()
        Ok((row.name, i64.MAX + row.id))
    result = unchecked:
        rows.sort_by_key(key)
    print(result.is_err(), len(rows))?
    drop(rows)
    mut numbers = [3, 1, 2]?
    good = numbers.sort_by_key(def(value: &i64) -> Result[i64, OverflowError[i64]]: Ok(*value + 1))
    print(good.is_ok(), numbers)?
    fallible = def(value: &i64) -> Result[str, str]:
        if *value == 2:
            return Err("a heap allocated callback error".repeat(10).unwrap())
        Ok("a heap allocated key".repeat(10).unwrap())
    owned_error = numbers.sort_by_key(fallible)
    print(owned_error.is_err())?
    drop(owned_error)
    Ok(())
"#,
    );
    assert!(output.starts_with("key 0\nkey 1\nTrue 3\n"), "{output}");
    for id in 0..3 {
        assert_eq!(
            output
                .lines()
                .filter(|line| *line == format!("drop {id}"))
                .count(),
            1,
            "{output}"
        );
    }
    assert!(output.ends_with("True [1, 2, 3]\nTrue\n"), "{output}");
}

/// Exercise the native adapter, not only frontend IR, in both backend modes.
fn key_sort_runs_in_both_modes(source: &str, expected: &str) {
    for optimize in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sort.plenty");
        let executable = directory.path().join("sort");
        std::fs::write(&path, source).unwrap();
        plenty::compile_file_to_executable_with_options(
            &path,
            &executable,
            None,
            &plenty::CompileOptions {
                optimize,
                test_hooks: true,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("optimize={optimize}: {error}\n{source}"));
        let output = support::output_within(
            &mut std::process::Command::new(executable),
            b"",
            std::time::Duration::from_secs(15),
        );
        assert!(
            output.status.success(),
            "optimize={optimize}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "optimize={optimize}"
        );
    }
}

#[test]
fn fallible_key_sort_relocates_range_and_nested_sum_errors() {
    key_sort_runs_in_both_modes(
        r#"
enum SortError:
    Span(Option[range[i64]])
    Message(str)
def span(value: &i64) -> Result[i64, range[i64]]:
    print("span", *value).unwrap()
    Err(range(123, 456))
def nested(value: &i64) -> Result[str, SortError]:
    print("nested", *value).unwrap()
    if *value == 1:
        return Err(SortError.Span(Some(range(123, 456))))
    Ok("a prebuilt owned key longer than seven bytes")
def main() -> Result[(), AllocError | IoError]:
    mut values = [0, 1, 2, 3, 4, 5, 6, 7]?
    match values.sort_by_key(span):
        case Err(interval):
            print(interval)?
        case Ok(_):
            print("missed error")?
    result = unchecked:
        values.sort_by_key(nested)
    match result:
        case Err(SortError.Span(Some(interval))):
            print(interval)?
        case _:
            print("wrong nested error")?
    print(values)?
    good = values.sort_by_key(def(value: &i64) -> Result[i64, OverflowError[i64]]: Ok(*value + 1), True)
    print(good.is_ok(), values)?
    overflow = values.sort_by_key(def(value: &i64) -> Result[i64, OverflowError[i64]]: Ok(i64.MAX + *value))
    match overflow:
        case Err(error):
            print(error.wrapped)?
        case Ok(_):
            print("missed overflow")?
    text = "heap error ".repeat(8)?
    heap = def [&text](value: &i64) -> Result[i64, str]:
        Err(text)
    match values.sort_by_key(heap):
        case Err(error):
            print(error == text)?
        case Ok(_):
            print("missed heap error")?
    Ok(())
"#,
        "span 0\nrange(123, 456, 1)\nnested 0\nnested 1\nrange(123, 456, 1)\n[0, 1, 2, 3, 4, 5, 6, 7]\nTrue [7, 6, 5, 4, 3, 2, 1, 0]\n-9223372036854775802\nTrue\n",
    );
}

#[test]
fn fallible_key_sort_relocates_managed_record_errors_and_drops_each_owner_once() {
    for (error_type, error_value, pattern) in [
        ("Problem", "problem", "problem"),
        (
            "SortError",
            "SortError.Record(problem)",
            "SortError.Record(problem)",
        ),
    ] {
        for fail_at in [0, 1] {
            let source = format!(
                r#"
from std.atomic import Atomic, Ordering
class Owner:
    text: str
    drops: Shared[Atomic[i64]]
    def __del__(self) -> ():
        drop(self.drops.fetch_add(1, Ordering.Relaxed))
class Problem:
    owner: Owner
    interval: range[i64]
    drops: Shared[Atomic[i64]]
    def __del__(self) -> ():
        drop(self.drops.fetch_add(1, Ordering.Relaxed))
enum SortError:
    Record(Problem)
    Span(range[i64])
    Message(str)
def main() -> Result[(), AllocError | IoError]:
    owner_drops = Shared(Atomic[i64](0))?
    error_drops = Shared(Atomic[i64](0))?
    calls = Atomic[i64](0)
    sequence = Atomic[i64](0)
    text = "owned payload ".repeat(8)?
    key_text = "owned first key ".repeat(8)?
    mut values = [0, 1, 2, 3, 4, 5, 6, 7]?
    key = def [&calls, &sequence, &text, &key_text, &owner_drops, &error_drops](value: &i64) -> Result[str, {error_type}]:
        call = calls.fetch_add(1, Ordering.Relaxed)
        if call == 0:
            sequence.store(*value, Ordering.Relaxed)
        else:
            sequence.store(sequence.load(Ordering.Relaxed).wrapping_mul(10).wrapping_add(*value), Ordering.Relaxed)
        if call == {fail_at}:
            problem = Problem(Owner(text, owner_drops.share()), range(123, 456), error_drops.share())
            return Err({error_value})
        Ok(key_text)
    print("__test_begin_no_allocations__").unwrap()
    result = unchecked:
        values.sort_by_key(key)
    print("__test_end_no_allocations__").unwrap()
    drop(key)
    drop(key_text)
    drop(text)
    print(calls.load(Ordering.Relaxed), sequence.load(Ordering.Relaxed), owner_drops.load(Ordering.Relaxed), error_drops.load(Ordering.Relaxed))?
    match result:
        case Err({pattern}):
            print(problem.owner.text, problem.interval)?
            drop(problem)
        case _:
            print("wrong error")?
    print(owner_drops.load(Ordering.Relaxed), error_drops.load(Ordering.Relaxed))?
    print(values)?
    Ok(())
"#
            );
            let payload = "owned payload ".repeat(8);
            key_sort_runs_in_both_modes(
                &source,
                &format!(
                    "__test_begin_no_allocations__\n__test_end_no_allocations__\n{} {fail_at} 0 0\n{payload} range(123, 456, 1)\n1 1\n[0, 1, 2, 3, 4, 5, 6, 7]\n",
                    fail_at + 1
                ),
            );
        }
    }
}

#[test]
fn fallible_key_sort_context_reserves_typed_inline_error_storage() {
    // Native output can accidentally survive expired comparator stack storage.
    // This layout oracle distinguishes that old implementation independently.
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("sort.plenty"),
        r#"
def key(value: &i64) -> Result[i64, range[i64]]:
    Err(range(123, 456))
def main() -> Result[(), AllocError]:
    mut values = [3, 1, 2]?
    drop(values.sort_by_key(key))
    Ok(())
"#,
    )
    .unwrap();
    for release in [false, true] {
        let mut compiler = std::process::Command::new(support::plenty_bin());
        compiler
            .current_dir(directory.path())
            .env("PLENTY_CACHE", "off");
        if !release {
            compiler.arg("--fast-compile");
        }
        support::assert_success(
            compiler
                .args(["--dump", "clif", "sort.plenty", "-o", "out"])
                .output()
                .unwrap(),
        );
        let ir = std::fs::read_to_string(directory.path().join("out/clif.txt")).unwrap();
        let sort = ir
            .split("\n==== ")
            .find(|section| section.lines().next().unwrap().contains("_sort "))
            .unwrap();
        // Callback word + Result[(), range[i64]] word + four range words.
        assert!(sort.contains("explicit_slot 64, align = 16"), "{sort}");
        let adapter = ir
            .split("\n==== ")
            .find(|section| section.lines().next().unwrap().contains("_order "))
            .unwrap();
        // Each key's failure path copies the entire four-word range before
        // returning, rather than publishing its comparator-local address.
        assert_eq!(
            adapter
                .lines()
                .filter(|line| line.contains("store notrap aligned") && line.ends_with("+24"))
                .count(),
            2,
            "{adapter}"
        );
    }
}

#[test]
fn closure_errors_cross_workers_without_lexical_effects() {
    assert_eq!(
        run_stdout(
            r#"
def main() -> Result[(), IoError]:
    created = checked:
        value = len("a") * 2
        def [value]() -> Result[i64, OverflowError[i64]]:
            Ok(i64.MAX + value)
    job = created.unwrap()
    with spawn(&job).unwrap() as task:
        print(task.join().is_err())?
    with ThreadPoolExecutor(1, 1).unwrap() as pool:
        future = pool.submit(job).unwrap()
        print(future.result().unwrap().is_err())?
    Ok(())
"#
        ),
        "True\nTrue\n"
    );
}

#[test]
fn fallible_folds_and_mutable_callbacks_propagate_only_their_public_errors() {
    assert_eq!(
        run_stdout(
            r#"
def forward[F: Callable[[i64], Result[i64, OverflowError[i64] | str]]](f: &mut F, x: i64) -> Result[i64, OverflowError[i64] | str]:
    value = f(x)?
    Ok(value)
def main() -> Result[(), AllocError | IoError]:
    mut count = 0
    mut f = def [&mut count](x: i64) -> Result[i64, OverflowError[i64] | str]:
        count += 1
        if x == 0:
            return Err("explicit")
        Ok(x + 1)
    print(forward(&mut f, 0).is_err(), forward(&mut f, 41).unwrap(), forward(&mut f, i64.MAX).is_err())?
    drop(f)
    print(count)?
    values = [i64.MAX, 1]?
    result = unchecked:
        iter(&values).try_fold(0, def(total: i64, value: &i64) -> Result[i64, OverflowError[i64]]: Ok(total + *value))
    print(result.is_err(), len(values))?
    Ok(())
"#
        ),
        "True 42 True\n3\nTrue 2\n"
    );
}

#[test]
fn reduction_failures_drop_owned_sources_and_resume_lent_sources_after_the_item() {
    assert_eq!(
        run_stdout(
            r#"
class Resource:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def values() -> Generator[i64]:
    owner = Resource(10)
    yield i64.MAX
    yield 1
    yield 2
def consume(source: &mut Iter[i64]) -> Result[i64, OverflowError[i64]]:
    source.sum()
def main() -> Result[(), AllocError | IoError]:
    print(iter(values()).sum().is_err())?
    mut source = iter([i64.MAX, 1, 2]?)
    print(consume(&mut source).is_err(), next(source).unwrap())?
    drop(source)
    Ok(())
"#
        ),
        "drop 10\nTrue\nTrue 2\n"
    );
}

#[test]
fn every_integer_width_uses_the_default_boundary() {
    let mut source = String::new();
    let mut calls = String::new();
    let mut expected = String::new();
    for ty in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        for (name, operation, args) in [
            ("add", "a + b", format!("{ty}.MAX, 1{ty}")),
            ("sub", "a - b", format!("{ty}.MIN, 1{ty}")),
            ("mul", "a * b", format!("{ty}.MAX, 2{ty}")),
            ("pow", "a.pow(b)", format!("2{ty}, 64{ty}")),
        ] {
            source.push_str(&format!("def {name}_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    Ok({operation})\n"));
            calls.push_str(&format!("    print({name}_{ty}({args}).is_err())?\n"));
            expected.push_str("True\n");
        }
        let signed = ty.starts_with('i');
        let errors = if signed {
            format!("ZeroDivisionError | OverflowError[{ty}]")
        } else {
            "ZeroDivisionError".into()
        };
        source.push_str(&format!(
            "def div_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, {errors}]:\n    Ok(a // b)\n"
        ));
        source.push_str(&format!(
            "def rem_{ty}(a: {ty}, b: {ty}) -> Result[{ty}, ZeroDivisionError]:\n    Ok(a % b)\n"
        ));
        calls.push_str(&format!(
            "    print(div_{ty}(1{ty}, 0{ty}).is_err(), rem_{ty}(1{ty}, 0{ty}).is_err())?\n"
        ));
        expected.push_str("True True\n");
        if signed {
            source.push_str(&format!(
                "def neg_{ty}(a: {ty}) -> Result[{ty}, OverflowError[{ty}]]:\n    Ok(-a)\n"
            ));
            calls.push_str(&format!("    print(neg_{ty}({ty}.MIN).is_err(), div_{ty}({ty}.MIN, -1{ty}).is_err(), rem_{ty}({ty}.MIN, -1{ty}).unwrap())?\n"));
            expected.push_str("True True 0\n");
        }
    }
    source.push_str(&format!(
        "def main() -> Result[(), IoError]:\n{calls}    Ok(())\n"
    ));
    assert_eq!(run_stdout(&source), expected);
}

#[cfg(feature = "runtime-checks")]
#[test]
fn implicit_arithmetic_failures_do_not_allocate() {
    let output = support::run(
        r#"
def step(a: i64, b: i64) -> Result[i64, OverflowError[i64] | ZeroDivisionError]:
    Ok(a * 2 // b)
def main() -> ():
    print("__test_begin_no_allocations__").unwrap()
    for divisor in range(-2, 3):
        drop(step(i64.MAX, divisor))
        drop(step(1, divisor))
    print("__test_end_no_allocations__").unwrap()
"#,
    );
    support::assert_success(output);
}

#[test]
fn unchecked_failure_terminates_instead_of_reaching_an_outer_boundary() {
    let source = r#"
def fail(n: i64) -> Result[i64, OverflowError[i64]]:
    result = checked:
        unchecked:
            drop(n + 1)
        n + 2
    result
def main() -> ():
    drop(fail(i64.MAX))
"#;
    for release in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trapping.plenty");
        let executable = dir.path().join("trapping");
        std::fs::write(&path, source).unwrap();
        let mut compiler = std::process::Command::new(support::plenty_bin());
        compiler.env("PLENTY_CACHE", "off");
        if !release {
            compiler.arg("--fast-compile");
        }
        let output = compiler
            .arg("--compile")
            .arg(&path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        support::assert_success(output);
        let output = std::process::Command::new(&executable).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "release={release}");
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("integer overflow"),
            "release={release}: {error}"
        );
    }
}

/// Unit bodies have no operand-stack success value; exercise native wrapping.
fn checked_unit_runs_in_both_modes(source: &str, expected: &str) {
    for optimize in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("checked-unit.plenty");
        let executable = directory.path().join("checked-unit");
        std::fs::write(&path, source).unwrap();
        plenty::compile_file_to_executable_with_options(
            &path,
            &executable,
            None,
            &plenty::CompileOptions {
                optimize,
                test_hooks: true,
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("optimize={optimize}: {error}\n{source}"));
        let output = support::output_within(
            &mut std::process::Command::new(executable),
            b"",
            std::time::Duration::from_secs(15),
        );
        assert!(
            output.status.success(),
            "optimize={optimize}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "optimize={optimize}"
        );
    }
}

#[test]
fn checked_unit_minimal_success_and_overflow_keep_the_compound_destination() {
    checked_unit_runs_in_both_modes(
        r#"
def cleanup(n: &mut i64) -> Result[(), OverflowError[i64]]:
    checked:
        *n += i64.MAX
        ()
def main() -> Result[(), IoError]:
    mut n = 0
    print(cleanup(&mut n).is_ok(), n)?
    n = 1
    match cleanup(&mut n):
        case Ok(_):
            print("missed overflow")?
        case Err(error):
            print(error.wrapped, n)?
    Ok(())
"#,
        "True 9223372036854775807\n-9223372036854775808 1\n",
    );
}

#[test]
fn checked_unit_preserves_earlier_mutation_and_handles_division_in_loop_branches() {
    checked_unit_runs_in_both_modes(
        r#"
class Point:
    x: i64
    y: i64
    def shift(self: &mut Point, amount: i64) -> Result[(), OverflowError[i64]]:
        checked:
            self.x = self.x + amount
            self.y = self.y + amount
            ()
def divide(n: &mut i64, divisor: i64) -> Result[(), OverflowError[i64] | ZeroDivisionError]:
    checked:
        *n //= divisor
        ()
def early(n: i64) -> Result[(), OverflowError[i64]]:
    checked:
        if n == 0:
            return Ok(())
        drop(n + 1)
        ()
def main() -> Result[(), IoError]:
    mut point = Point(1, i64.MAX)
    print(point.shift(1).is_err(), point.x, point.y)?
    for divisor in range(0, 3):
        mut n = 12
        result = if divisor == 0:
            divide(&mut n, divisor)
        else:
            checked:
                if divisor == 1:
                    continue
                n //= divisor
                ()
        match result:
            case Ok(_):
                print("ok", n)?
            case Err(ZeroDivisionError()):
                print("zero", n)?
            case Err(OverflowError(wrapped)):
                print("overflow", wrapped, n)?
    mut n = i64.MIN
    print(divide(&mut n, -1).is_err(), n)?
    print(early(0).is_ok(), early(1).is_ok(), early(i64.MAX).is_err())?
    Ok(())
"#,
        "True 2 9223372036854775807\nzero 12\nok 6\nTrue -9223372036854775808\nTrue True True\n",
    );
}

#[test]
fn checked_unit_drops_owners_and_pending_temporaries_once_and_propagates() {
    checked_unit_runs_in_both_modes(
        r#"
class Owner:
    id: i64
    def __del__(self: &mut Owner) -> ():
        print("drop", self.id).unwrap()
def accept(owner: Owner, n: i64) -> ():
    print("accept", owner.id, n).unwrap()
def parse(fail: bool) -> Result[(), ParseError]:
    if fail:
        return Err(ParseError.Invalid)
    Ok(())
def cleanup(n: i64, fail: bool) -> Result[(), AllocError | OverflowError[i64] | ParseError]:
    outer = Owner(1)
    result = checked:
        local = Owner(2)
        items = [1, 2, 3]?
        accept(Owner(3), n + i64.MAX)
        parse(fail)?
        drop(local)
        drop(items)
        ()
    result?
    print("after").unwrap()
    Ok(())
def main() -> Result[(), IoError]:
    print(cleanup(0, False).is_ok())?
    print(cleanup(1, False).is_err())?
    print(cleanup(0, True).is_err())?
    Ok(())
"#,
        "accept 3 9223372036854775807\ndrop 3\ndrop 2\nafter\ndrop 1\nTrue\ndrop 3\ndrop 2\ndrop 1\nTrue\naccept 3 9223372036854775807\ndrop 3\ndrop 2\ndrop 1\nTrue\n",
    );
}

#[test]
fn checked_unit_fix_keeps_float_checked_division_with_pending_operands() {
    checked_unit_runs_in_both_modes(
        r#"
def divide(a: f64, b: f64) -> Result[f64, ZeroDivisionError]:
    Ok(10.0 + a.div_checked(b)?)
def main() -> Result[(), IoError]:
    print(divide(12.0, 3.0).unwrap(), divide(12.0, 0.0).is_err(), divide(12.0, -0.0).is_err())?
    Ok(())
"#,
        "14.0 True True\n",
    );
}
