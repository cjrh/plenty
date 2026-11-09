//! Generators that yield shared references (issue #912): each item borrows
//! the data the frame borrows, so it stays valid after the generator advances
//! or is dropped, and the source cannot change while an item is live.

mod support;

use support::{assert_stdout, check_error};

const NAMES: &str = r#"
def names(xs: &list[str]) -> Generator[&str]:
    for x in xs:
        yield x

def relay(g: Generator[&str]) -> Generator[&str]:
    for v in g:
        yield v
"#;

fn with_names(body: &str) -> String {
    format!("{NAMES}\n{body}")
}

fn main_body(body: &str) -> String {
    with_names(&format!(
        "def main() -> Result[(), AllocError | IoError]:\n{}\n    Ok(())\n",
        body.lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

#[test]
fn items_outlive_advancing_and_dropping_the_generator() {
    assert_stdout(
        &main_body(
            r#"xs = ["a", "b", "c"]?
mut g = names(&xs)
a = next(g)
b = next(g)
drop(g)
print(a)?
print(b)?
for v in names(&xs):
    print(v)?"#,
        ),
        "Option[&str].Some(\"a\")\nOption[&str].Some(\"b\")\na\nb\nc\n",
    );
}

#[test]
fn the_source_cannot_change_while_an_item_is_live() {
    for (change, expected) in [
        ("xs.append(\"c\")?", "borrowed"),
        ("xs.clear()", "borrowed"),
        ("drop(xs)", "use of moved binding `xs`"),
    ] {
        let error = check_error(&main_body(&format!(
            r#"mut xs = ["a", "b"]?
mut g = names(&xs)
x = next(g)
drop(g)
{change}
print(x)?"#
        )));
        assert!(error.contains(expected), "{change}: {error}");
    }
}

#[test]
fn an_item_kept_from_a_loop_keeps_the_source_borrowed() {
    for iterable in ["names(&xs)", "names(&xs).skip(0)", "relay(names(&xs))"] {
        let error = check_error(&main_body(&format!(
            r#"mut xs = ["a", "b"]?
mut keep: Option[&str] = Nothing
for v in {iterable}:
    keep = Some(v)
xs.clear()
print(keep)?"#
        )));
        assert!(error.contains("borrowed"), "{iterable}: {error}");
    }
    assert_stdout(
        &main_body(
            r#"xs = ["a", "b"]?
mut keep: Option[&str] = Nothing
for v in names(&xs):
    keep = Some(v)
print(keep)?"#,
        ),
        "Option[&str].Some(\"b\")\n",
    );
}

#[test]
fn pipelines_keep_borrowed_items() {
    assert_stdout(
        &main_body(
            r#"xs = ["a", "bb"]?
ys = ["c", "d"]?
for a, b in zip(names(&xs), names(&ys)):
    print(a)?
    print(b)?
for i, v in enumerate(names(&xs)):
    print(v)?
print(names(&xs).count())?"#,
        ),
        "a\nc\nbb\nd\na\nbb\n2\n",
    );
}

#[test]
fn generators_relay_borrowed_items() {
    assert_stdout(
        &with_names(
            r#"
def pairs(xs: &list[str], ys: &list[str]) -> Generator[&str]:
    for a, b in zip(names(xs), names(ys)):
        yield a
        yield b

def through(p: &mut Generator[&str]) -> Generator[&str]:
    for v in p:
        yield v

def main() -> Result[(), AllocError | IoError]:
    xs = ["a", "bb"]?
    ys = ["c", "d"]?
    for v in pairs(&xs, &ys):
        print(v)?
    mut g = names(&xs)
    for v in through(&mut g):
        print(v)?
    mut r = relay(names(&ys))
    x = next(r)
    drop(r)
    print(x)?
    Ok(())
"#,
        ),
        "a\nc\nbb\nd\na\nbb\nOption[&str].Some(\"c\")\n",
    );
}

#[test]
fn functions_return_borrowed_items_and_generators() {
    assert_stdout(
        &with_names(
            r#"
def first(xs: &list[str]) -> Option[&str]:
    mut g = names(xs)
    next(g)

def take(p: &mut Generator[&str]) -> Option[&str]:
    next(p)

def all_of(xs: &list[str]) -> Generator[&str]:
    names(xs)

def main() -> Result[(), AllocError | IoError]:
    xs = ["a", "b"]?
    print(first(&xs))?
    mut g = names(&xs)
    print(take(&mut g))?
    print(all_of(&xs).count())?
    Ok(())
"#,
        ),
        "Option[&str].Some(\"a\")\nOption[&str].Some(\"a\")\n2\n",
    );
}

#[test]
fn a_yielded_reference_must_borrow_the_caller_data() {
    for (source, expected) in [
        (
            "def bad(n: i64) -> Generator[&i64]:\n    x = n\n    yield &x",
            "yielded reference",
        ),
        (
            "def bad(xs: list[i64]) -> Generator[&i64]:\n    for x in &xs:\n        yield x",
            "yielded reference",
        ),
        (
            "def bad(xs: &mut list[i64]) -> Generator[&i64]:\n    yield &xs[0]",
            "yielded reference",
        ),
        // Items of a generator over a `&mut` parameter: the frame could
        // clear `xs` while the caller holds them.
        (
            "def bad(xs: &mut list[str]) -> Generator[&str]:\n    for x in names(xs):\n        yield x\n    xs.clear()",
            "yielded reference",
        ),
        (
            "def bad(xs: &mut list[str]) -> Generator[&str]:\n    mut g = names(&*xs)\n    match next(g):\n        case Some(x):\n            yield x\n        case Nothing:\n            pass\n    xs.clear()",
            "yielded reference",
        ),
        (
            "def bad(xs: &mut list[str]) -> Generator[&str]:\n    r = &*xs\n    for a, b in zip(names(r), relay(names(r))):\n        yield b",
            "yielded reference",
        ),
        (
            "def bad(xs: &mut list[i64]) -> Generator[&mut i64]:\n    for x in xs:\n        yield x",
            "generators cannot yield mutable references",
        ),
    ] {
        let error = check_error(&with_names(&format!(
            "{source}\n\ndef main() -> ():\n    pass\n"
        )));
        assert!(error.contains(expected), "{source}: {error}");
    }
}

#[test]
fn a_caller_cannot_keep_an_item_its_frame_may_invalidate() {
    let error = check_error(&with_names(
        r#"
def bad(xs: &mut list[str]) -> Generator[&str]:
    for x in names(xs):
        yield x
    xs.clear()

def main() -> Result[(), AllocError | IoError]:
    mut xs = ["a", "b"]?
    mut keep: Option[&str] = Nothing
    for v in bad(&mut xs):
        keep = Some(v)
    print(keep)?
    Ok(())
"#,
    ));
    assert!(error.contains("yielded reference"), "{error}");
}

#[test]
fn a_generator_relays_items_through_mutable_generator_references() {
    assert_stdout(
        &with_names(
            r#"
def inner(p: &mut Generator[&str]) -> Generator[&str]:
    for v in p:
        yield v

def outer(p: &mut Generator[&str]) -> Generator[&str]:
    for v in inner(p):
        yield v

def main() -> Result[(), AllocError | IoError]:
    xs = ["a", "b"]?
    mut g = names(&xs)
    for v in outer(&mut g):
        print(v)?
    Ok(())
"#,
        ),
        "a\nb\n",
    );
}

#[test]
fn a_relay_through_mutable_generator_references_keeps_the_source_borrowed() {
    let error = check_error(&with_names(
        r#"
def inner(p: &mut Generator[&str]) -> Generator[&str]:
    for v in p:
        yield v

def outer(p: &mut Generator[&str]) -> Generator[&str]:
    for v in inner(p):
        yield v

def main() -> Result[(), AllocError | IoError]:
    mut xs = ["a", "b"]?
    mut g = names(&xs)
    mut o = outer(&mut g)
    a = next(o)
    xs.append("c")?
    b = next(o)
    print(a)?
    print(b)?
    Ok(())
"#,
    ));
    assert!(error.contains("borrowed"), "{error}");
}

const TWICE: &str = r#"
def twice(g: &mut Generator[&str]) -> Generator[&str]:
    match next(g):
        case Some(x):
            yield x
            match next(g):
                case Some(y):
                    yield y
                case Nothing:
                    pass
            yield x
        case Nothing:
            pass
"#;

#[test]
fn an_item_taken_by_next_through_a_mutable_generator_reference_stays_live_across_yield() {
    // Issue #922: the item borrows the caller's source, not the frame.
    assert_stdout(
        &with_names(&format!(
            r#"{TWICE}
def main() -> Result[(), AllocError | IoError]:
    xs = ["a", "b"]?
    mut g = names(&xs)
    for v in twice(&mut g):
        print(v)?
    Ok(())
"#
        )),
        "a\nb\na\n",
    );
}

#[test]
fn an_item_held_across_yield_keeps_the_callers_source_borrowed() {
    let error = check_error(&with_names(&format!(
        r#"{TWICE}
def main() -> Result[(), AllocError | IoError]:
    mut xs = ["a", "b"]?
    mut g = names(&xs)
    mut t = twice(&mut g)
    a = next(t)
    xs.clear()
    b = next(t)
    print(a)?
    print(b)?
    Ok(())
"#
    )));
    assert!(error.contains("`xs` while it is borrowed"), "{error}");
}

#[test]
fn an_item_kept_after_dropping_the_relay_keeps_the_callers_source_borrowed() {
    let error = check_error(&with_names(&format!(
        r#"{TWICE}
def main() -> Result[(), AllocError | IoError]:
    mut xs = ["a", "b"]?
    mut g = names(&xs)
    mut t = twice(&mut g)
    a = next(t)
    drop(t)
    xs.clear()
    print(a)?
    Ok(())
"#
    )));
    assert!(error.contains("`xs` while it is borrowed"), "{error}");
}

#[test]
fn reference_items_cannot_be_stored_in_collections() {
    let error = check_error(&main_body(
        r#"xs = ["a", "b"]?
ys = list(names(&xs))?
print(ys)?"#,
    ));
    assert!(error.contains("reference"), "{error}");
}

#[test]
fn a_borrowing_generator_cannot_be_replaced_through_a_reference() {
    // Writing a new frame through `&mut Generator` would let the caller's
    // binding hold loans the caller never sees.
    let error = check_error(&with_names(
        r#"
def bad(p: &mut Generator[&str], ys: &list[str]) -> Generator[&str]:
    *p = names(ys)
    for v in p:
        yield v

def main() -> Result[(), AllocError | IoError]:
    ys = ["a"]?
    mut g = names(&ys)
    for v in bad(&mut g, &ys):
        print(v)?
    Ok(())
"#,
    ));
    assert!(error.contains("assigned through a reference"), "{error}");
}

#[test]
fn retargeting_a_copy_of_a_generator_parameter_is_rejected() {
    let error = check_error(&with_names(
        r#"
def bad(g: Generator[&str]) -> Generator[&str]:
    mut h = g
    match ["z"]:
        case Ok(xs):
            h = names(&xs)
            for v in h:
                yield v
        case Err(e):
            pass

def main() -> Result[(), AllocError | IoError]:
    ys = ["a"]?
    for v in bad(names(&ys)):
        print(v)?
    Ok(())
"#,
    ));
    assert!(!error.is_empty());
}

const FIRST: &str = r#"
def first(g: Generator[&str]) -> Option[&str]:
    mut h = g
    next(h)

def last(g: Generator[&str]) -> Option[&str]:
    mut keep: Option[&str] = Nothing
    for v in g:
        keep = Some(v)
    keep
"#;

fn with_first(body: &str) -> String {
    main_body(body).replacen(NAMES, &format!("{NAMES}{FIRST}"), 1)
}

#[test]
fn an_item_returns_through_a_by_value_generator_parameter() {
    // Issue #921: the item borrows the generator's sources, not its frame.
    assert_stdout(
        &with_first(
            r#"xs = ["a", "b"]?
print(first(names(&xs)))?
print(last(relay(names(&xs))))?
g = names(&xs)
f = first(g)
print(f)?"#,
        ),
        "Option[&str].Some(\"a\")\nOption[&str].Some(\"b\")\nOption[&str].Some(\"a\")\n",
    );
}

#[test]
fn a_returned_item_keeps_the_generator_sources_borrowed() {
    for call in ["first(names(&xs))", "last(relay(names(&xs)))"] {
        let error = check_error(&with_first(&format!(
            r#"mut xs = ["a", "b"]?
r = {call}
xs.clear()
print(r)?"#
        )));
        assert!(error.contains("borrowed"), "{call}: {error}");
    }
}

#[test]
fn a_returned_item_keeps_a_mut_source_of_the_frame_borrowed() {
    let source = |change: &str| {
        format!(
            r#"
def tagged(log: &mut list[i64], xs: &list[str]) -> Generator[&str]:
    for x in xs:
        log[0] = 1
        yield x

def first(g: Generator[&str]) -> Option[&str]:
    mut h = g
    next(h)

def main() -> Result[(), AllocError | IoError]:
    mut log = [0]?
    xs = ["a", "b"]?
    r = first(tagged(&mut log, &xs))
    {change}
    print(r)?
    Ok(())
"#
        )
    };
    let error = check_error(&source("log[0] = 5"));
    assert!(error.contains("while `log` is borrowed"), "{error}");
    assert_stdout(&source("print(log[0])?"), "1\nOption[&str].Some(\"a\")\n");
}

#[test]
fn a_by_value_generator_parameter_is_the_origin_in_parameter_order() {
    let error = check_error(&with_names(
        r#"
def pick(g: Generator[&str], s: &str) -> &str:
    s

def main() -> Result[(), AllocError | IoError]:
    xs = ["a"]?
    print(pick(names(&xs), "b"))?
    Ok(())
"#,
    ));
    assert!(error.contains("first reference parameter, `g`"), "{error}");
    let error = check_error(&with_names(
        r#"
def pick(s: &str, g: Generator[&str]) -> Option[&str]:
    mut h = g
    next(h)

def main() -> Result[(), AllocError | IoError]:
    xs = ["a"]?
    print(pick("b", names(&xs)))?
    Ok(())
"#,
    ));
    assert!(error.contains("first reference parameter, `s`"), "{error}");
    // A later reference argument's loan ends with the call.
    assert_stdout(
        &with_names(
            r#"
def pick(g: Generator[&str], s: &str) -> Option[&str]:
    for v in g:
        if v == s:
            return Some(v)
    Nothing

def main() -> Result[(), AllocError | IoError]:
    xs = ["a", "b"]?
    mut ys = ["b"]?
    r = pick(names(&xs), ys[0])
    ys.clear()
    print(r)?
    Ok(())
"#,
        ),
        "Option[&str].Some(\"b\")\n",
    );
}

#[test]
fn a_by_value_generator_origin_rejects_other_returns() {
    for (output, body, expected) in [
        (
            "Option[&str]",
            "s = \"x\"\n    Some(&s)",
            "originate from the first reference parameter, `g`",
        ),
        ("Option[&mut str]", "Nothing", "to be `&mut`"),
    ] {
        let error = check_error(&with_names(&format!(
            r#"
def bad(g: Generator[&str]) -> {output}:
    {body}

def main() -> Result[(), AllocError | IoError]:
    xs = ["a"]?
    print(bad(names(&xs)))?
    Ok(())
"#
        )));
        assert!(error.contains(expected), "{output}: {error}");
    }
}

#[test]
fn a_generic_function_returns_an_item_of_a_by_value_generator() {
    let source = |order: &str| {
        format!(
            r#"
def items[T](xs: &list[T]) -> Generator[&T]:
    for x in xs:
        yield x

def first[T](g: Generator[&T]) -> Option[&T]:
    mut h = g
    next(h)

def main() -> Result[(), AllocError | IoError]:
    mut xs = [1, 2]?
    r = first(items(&xs))
    {order}
    Ok(())
"#
        )
    };
    assert_stdout(
        &source("print(r)?\n    xs.clear()"),
        "Option[&i64].Some(1)\n",
    );
    let error = check_error(&source("xs.clear()\n    print(r)?"));
    assert!(error.contains("borrowed"), "{error}");
}

#[test]
fn a_generator_yields_an_item_returned_through_its_generator_parameter() {
    // A generator body's by-value generator parameter has no loan of its
    // own; the returned item is rooted at the parameter, as `next` is.
    assert_stdout(
        &main_body(
            r#"for v in heads(names(&xs)):
    print(v)?"#,
        )
        .replacen(
            NAMES,
            &format!(
                "{NAMES}{FIRST}
def heads(g: Generator[&str]) -> Generator[&str]:
    match first(g):
        case Some(v):
            yield v
        case Nothing:
            pass
"
            ),
            1,
        )
        .replacen(
            "    for v in heads",
            "    xs = [\"a\", \"b\"]?\n    for v in heads",
            1,
        ),
        "a\n",
    );
}

#[test]
fn a_generator_yields_an_item_returned_through_a_relayed_or_generic_call() {
    // Each call path roots its argument loan at the generator body's own
    // generator parameter, which has no loan (issue #921).
    for call in ["first(relay(g))", "first_of(g)"] {
        let source = main_body(
            r#"xs = ["a", "b"]?
for v in heads(names(&xs)):
    print(v)?"#,
        )
        .replacen(
            NAMES,
            &format!(
                "{NAMES}{FIRST}
def first_of[T](g: Generator[&T]) -> Option[&T]:
    mut h = g
    next(h)

def heads(g: Generator[&str]) -> Generator[&str]:
    match {call}:
        case Some(v):
            yield v
        case Nothing:
            pass
"
            ),
            1,
        );
        assert_stdout(&source, "a\n");
    }
}
