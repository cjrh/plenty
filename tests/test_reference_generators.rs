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
