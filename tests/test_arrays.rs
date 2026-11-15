//! Fixed inline arrays use dense elements and ordinary owner-local aggregate ABI.
mod support;
fn run(source: &str, expected: &str) {
    for optimize in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("arrays");
        plenty::compile_source_to_executable_with_options(
            source,
            &executable,
            &plenty::CompileOptions {
                optimize,
                ..Default::default()
            },
        )
        .unwrap();
        let output = std::process::Command::new(&executable).output().unwrap();
        assert!(
            output.status.success(),
            "status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
    }
}
#[test]
fn literals_index_borrow_mutate_and_value_return() {
    run(
        r#"
def make() -> array[i64, 3]:
    [10, 20, 30]
def read(xs: &array[i64, 3], i: i64) -> i64:
    xs[i]
def main() -> Result[(), IoError]:
    mut xs = make()
    mut i = -1
    print(len(xs), read(&xs, -1), xs, xs[-1], xs[i])?
    i = 0
    last = make()[-1]
    print(xs[i], make()[-1], last, array[i64, 2].filled(7)[-1])?
    xs[1] = 99
    print(xs[1], xs)?
    Ok(())
"#,
        "3 30 array[10, 20, 30] 30 30\n10 30 30 7\n99 array[10, 99, 30]\n",
    );
}

#[test]
fn observed_temporary_array_keeps_owned_elements_alive() {
    run(
        r#"
class Item:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
def make() -> array[Item, 2]:
    [Item(1), Item(2)]
def main() -> Result[(), IoError]:
    print(make()[-1])?
    Ok(())
"#,
        "Item(id=2)\ndrop 1\ndrop 2\n",
    );
}
#[test]
fn packed_nested_array_neighbor_survives_replacement() {
    run(
        r#"
def first(xs: array[u8, 3]) -> array[u8, 3]:
    xs
def main() -> Result[(), IoError]:
    mut xs: array[array[u8, 3], 2] = [[1, 2, 3], [4, 5, 6]]
    xs[0] = first(xs[1])
    print(xs, xs[1][2])?
    Ok(())
"#,
        "array[array[4, 5, 6], array[4, 5, 6]] 6\n",
    );
}
#[test]
fn invalid_counts_and_borrowed_elements_rejected() {
    for (ty, message) in [
        ("array[i64, -1]", "nonnegative"),
        ("array[i64, 9223372036854775807]", "inline storage limit"),
        ("array[&i64, 0]", "references cannot be stored"),
    ] {
        let source = format!("def use(x: {ty}) -> ():\n    pass\ndef main() -> ():\n    pass\n");
        let error = plenty::check_source(&source).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}

#[test]
fn factories_zero_and_borrowed_iteration() {
    run(
        r#"
def item(i: i64) -> i64:
    unchecked:
        i * 10
def main() -> Result[(), IoError]:
    xs = array[i64, 3].from_fn(item)
    for x in &xs:
        print(*x)?
    mut ys = array[i64, 2].filled(5)
    for y in &mut ys:
        *y = 8
    print(ys, array[i64, 0].filled(99), xs == xs)?
    Ok(())
"#,
        "0\n10\n20\narray[8, 8] array[] True\n",
    );
}
#[test]
fn generic_array_shape_infers_type_count_and_nested_shapes() {
    run(
        r#"
def last[T, const N: i64](xs: array[T, N]) -> T:
    xs[-1]
def total[const N: i64](xs: array[u8, N]) -> i64:
    i64(xs[-1])
def nested[T, const N: i64, const M: i64](xs: array[array[T, N], M]) -> array[T, N]:
    xs[-1]
def main() -> Result[(), IoError]:
    print(last([10, 20, 30]), total([1, 2, 255]))?
    result = nested([[1, 2], [3, 4]])
    print(result)?
    Ok(())
"#,
        "30 255\narray[3, 4]\n",
    );
}
#[test]
fn array_owners_drop_once_and_literal_error_cleans_prefix() {
    run(
        r#"
class Item:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
@copy
enum Error:
    Failed
def make(i: i64) -> Result[Item, Error]:
    if i == 3:
        Err(Error.Failed)
    else:
        Ok(Item(i))
def attempt() -> Result[array[Item, 3], Error]:
    Ok([make(1)?, make(2)?, make(3)?])
def main() -> Result[(), IoError]:
    match attempt():
        case Ok(_):
            print("unexpected")?
        case Err(_):
            print("failed")?
    Ok(())
"#,
        "drop 1\ndrop 2\nfailed\n",
    );
}

#[test]
fn fallible_factory_cleans_only_initialized_prefix() {
    run(
        r#"
class Item:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
@copy
enum Error:
    Failed
def item(i: i64) -> Result[Item, Error]:
    if i == 2:
        Err(Error.Failed)
    else:
        Ok(Item(i))
def main() -> Result[(), IoError]:
    match array[Item, 4].try_from_fn(item):
        case Ok(_):
            print("unexpected")?
        case Err(_):
            print("failed")?
    Ok(())
"#,
        "drop 0\ndrop 1\nfailed\n",
    );
}

#[test]
fn zero_array_evaluates_filled_argument_but_never_calls_factory() {
    run(
        r#"
def argument() -> i64:
    print("argument").unwrap()
    7
def factory(i: i64) -> i64:
    print("factory", i).unwrap()
    i
def main() -> Result[(), IoError]:
    a = array[i64, 0].filled(argument())
    b = array[i64, 0].from_fn(factory)
    print(len(a), a == b)?
    Ok(())
"#,
        "argument\n0 True\n",
    );
}

#[test]
fn generic_shapes_propagate_through_reference_and_tuple() {
    run(
        r#"
def borrowed[T, const N: i64](xs: &array[T, N]) -> i64:
    len(xs)
def paired[T, const N: i64](xs: (array[T, N], i64)) -> i64:
    len(xs[0])
def main() -> Result[(), IoError]:
    print(borrowed(&[1, 2, 3]), paired(([4, 5], 7)))?
    Ok(())
"#,
        "3 2\n",
    );
}

#[test]
fn unit_factory_and_nested_zero_array() {
    run(
        r#"
def unit(i: i64) -> ():
    print(i).unwrap()
def main() -> Result[(), IoError]:
    xs = array[(), 2].from_fn(unit)
    ys = array[array[i64, 0], 2].filled(array[i64, 0].filled(7))
    print(xs, ys, len(ys))?
    Ok(())
"#,
        "0\n1\narray[(), ()] array[array[], array[]] 2\n",
    );
}

#[test]
fn element_loans_and_zero_length_ownership_restrictions() {
    for (source, expected) in [
        (
            r#"
def make() -> array[i64, 2]:
    [1, 2]
def main() -> ():
    item = &make()[0]
    print(*item).unwrap()
"#,
            "temporary cannot be stored",
        ),
        (
            r#"
def main() -> ():
    mut xs: array[i64, 2] = [1, 2]
    item = &mut xs[0]
    xs[0] = 7
    print(*item).unwrap()
"#,
            "borrow",
        ),
        (
            r#"
class Item:
    id: i64
    def __del__(self) -> ():
        pass
def main() -> ():
    xs = array[Item, 0].filled(Item(1))
"#,
            "copyable element",
        ),
        (
            r#"
def main() -> ():
    xs: array[str, 2] = ["a", "b"]
    print(",".join(xs).unwrap()).unwrap()
"#,
            "join requires",
        ),
        (
            r#"
def main() -> ():
    xs: array[i64, 2] = [1, 2]
    ys = list(xs).unwrap()
"#,
            "requires a borrow",
        ),
    ] {
        let error = plenty::check_source(source).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn arrays_in_owned_records_sums_lists_and_explicit_copy() {
    run(
        r#"
class Block:
    cells: array[list[i64], 2]
    flag: i64
def make() -> Result[Block, AllocError]:
    Ok(Block([[1]?, [2]?], 9))
def wrap(value: array[u8, 3]) -> Option[array[u8, 3]]:
    Some(value)
def main() -> Result[(), IoError | AllocError]:
    mut block = make()?
    duplicate = copy(block)?
    block.cells[0].append(7)?
    print(block, duplicate)?
    entries: list[array[u8, 3]] = [[1, 2, 3], [4, 5, 6]]?
    print(entries, wrap([8, 9, 10]).unwrap())?
    Ok(())
"#,
        "Block(cells=array[[1, 7], [2]], flag=9) Block(cells=array[[1], [2]], flag=9)\n[array[1, 2, 3], array[4, 5, 6]] array[8, 9, 10]\n",
    );
}

#[test]
fn reverse_iteration_and_disjoint_constant_element_loans() {
    run(
        r#"
def main() -> Result[(), IoError]:
    mut xs: array[i64, 3] = [1, 2, 3]
    first = &mut xs[0]
    last = &mut xs[2]
    *first = 4
    *last = 5
    for x in reversed(&xs):
        print(*x)?
    Ok(())
"#,
        "5\n2\n4\n",
    );
}

#[test]
fn nested_fallible_factories_release_both_prefixes_once() {
    run(
        r#"
class Item:
    id: i64
    def __del__(self) -> ():
        print("drop", self.id).unwrap()
@copy
enum Error:
    Failed
def good(i: i64) -> Result[Item, Error]:
    Ok(Item(i))
def failing(i: i64) -> Result[Item, Error]:
    if i == 1:
        Err(Error.Failed)
    else:
        Ok(Item(10))
def outer(i: i64) -> Result[array[Item, 2], Error]:
    if i == 1:
        array[Item, 2].try_from_fn(failing)
    else:
        array[Item, 2].try_from_fn(good)
def main() -> Result[(), IoError]:
    match array[array[Item, 2], 3].try_from_fn(outer):
        case Ok(_):
            print("unexpected")?
        case Err(_):
            print("failed")?
    Ok(())
"#,
        "drop 10\ndrop 0\ndrop 1\nfailed\n",
    );
}

#[test]
fn array_bounds_traps_cover_empty_positive_and_negative_indices() {
    for (count, index) in [(0, 0), (0, -1), (3, 3), (3, -4)] {
        let source = format!(
            "def read(xs: &array[i64, {count}], i: i64) -> i64:\n    xs[i]\ndef main() -> Result[(), IoError]:\n    xs = array[i64, {count}].filled(7)\n    print(read(&xs, {index}))?\n    Ok(())\n"
        );
        for optimize in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let executable = directory.path().join("trap");
            plenty::compile_source_to_executable_with_options(
                &source,
                &executable,
                &plenty::CompileOptions {
                    optimize,
                    ..Default::default()
                },
            )
            .unwrap();
            let output = std::process::Command::new(executable).output().unwrap();
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains(&format!(
                    "index out of bounds: index {index}, length {count}"
                )),
                "{error}"
            );
        }
    }
}

#[test]
fn aliases_and_reusable_factory_captures() {
    run(
        r#"
type Block = array[i64, 3]
def main() -> Result[(), IoError]:
    offset = 7
    callback = def [offset](i: i64) -> i64:
        unchecked:
            offset + i
    block = Block.from_fn(callback)
    print(block, Block.filled(4))?
    Ok(())
"#,
        "array[7, 8, 9] array[4, 4, 4]\n",
    );
}

#[test]
fn reusable_borrowed_factory_retains_capture_loan_through_loop() {
    run(
        r#"
def main() -> Result[(), IoError]:
    mut calls = 0
    mut factory = def [&mut calls](i: i64) -> i64:
        unchecked:
            calls += 1
        i
    xs = array[i64, 3].from_fn(factory)
    print(xs, calls)?
    Ok(())
"#,
        "array[0, 1, 2] 3\n",
    );
}

#[test]
fn forward_nominal_elements_are_laid_out_after_definitions_finish() {
    run(
        r#"
class Holder:
    values: array[Item, 2]
class Item:
    value: i64
def main() -> Result[(), IoError]:
    holder = Holder([Item(3), Item(4)])
    print(holder)?
    Ok(())
"#,
        "Holder(values=array[Item(value=3), Item(value=4)])\n",
    );
    let error = plenty::check_source(
        "class Recursive:\n    values: array[Recursive, 0]\ndef main() -> ():\n    pass\n",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("infinite size"), "{error}");
}

#[test]
fn all_primitive_widths_use_dense_scalar_fields() {
    for (ty, value, expected) in [
        ("i8", "-7", "-7"),
        ("u8", "255", "255"),
        ("i16", "-300", "-300"),
        ("u16", "65535", "65535"),
        ("i32", "-70000", "-70000"),
        ("u32", "4000000000", "4000000000"),
        ("i64", "-9000000000", "-9000000000"),
        ("u64", "10000000000", "10000000000"),
        ("f32", "1.5", "1.5"),
        ("f64", "2.5", "2.5"),
        ("bool", "True", "True"),
    ] {
        let source = format!(
            "def main() -> Result[(), IoError]:\n    xs = array[{ty}, 3].filled({value})\n    print(xs[1], xs)?\n    Ok(())\n"
        );
        run(
            &source,
            &format!("{expected} array[{expected}, {expected}, {expected}]\n"),
        );
    }
}

#[test]
fn zero_length_factory_still_requires_a_reusable_callback() {
    let source = r#"
def main() -> Result[(), AllocError]:
    items = [1]?
    once = def once [items](i: i64) -> list[i64]:
        items
    empty = array[list[i64], 0].from_fn(once)
    Ok(())
"#;
    let error = plenty::check_source(source).unwrap_err().to_string();
    assert!(error.contains("reusable callback"), "{error}");
}
