//! `Result`/`Option` predicates, integer rotation, and `str.byte_len` (issue #116).
mod support;

fn output(source: &str) -> String {
    let output = support::run(source);
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn rejected(source: &str) -> String {
    support::check_source(source)
        .expect_err("source should be rejected")
        .to_string()
}

#[test]
fn predicates_report_the_variant() {
    assert_eq!(
        output(
            r#"
ok: Result[i64, str] = Ok(1)
bad: Result[i64, str] = Err("no")
some: Option[i64] = Some(0)
none: Option[i64] = Nothing
print(ok.is_ok(), ok.is_err(), bad.is_ok(), bad.is_err()).unwrap()
print(some.is_some(), some.is_none(), none.is_some(), none.is_none()).unwrap()
unit: Result[(), str] = Ok(())
empty: Option[()] = Some(())
print(unit.is_ok(), empty.is_some()).unwrap()
"#
        ),
        "True False False True\nTrue False False True\nTrue True\n"
    );
}

#[test]
fn predicates_borrow_owned_receivers() {
    assert_eq!(
        output(
            r#"
class Holder:
    slot: Option[str]

def build(n: i64) -> Result[list[i64], AllocError]:
    mut xs = [n, n]?
    if n < 0:
        xs.reserve(-1)?
    Ok(xs)

def both(r: &Result[list[i64], AllocError], o: &Option[str]) -> bool:
    r.is_ok() and o.is_some()

def main() -> ():
    built = build(3)
    print(built.is_ok(), built.is_err()).unwrap()
    text: Option[str] = Some("long enough to need heap storage")
    print(text.is_some(), text.is_none()).unwrap()
    print(both(&built, &text)).unwrap()
    print(built.unwrap(), text.unwrap_or("")).unwrap()
    print(build(-1).is_err(), build(1).is_ok()).unwrap()
    holder = Holder(Some("another string kept on the heap"))
    if holder.slot.is_some() and not holder.slot.is_none():
        print(holder.slot.unwrap_or("")).unwrap()
    boxed = Box(build(2)).unwrap()
    print(boxed.is_ok()).unwrap()
"#
        ),
        "True False\nTrue False\nTrue\n[3, 3] long enough to need heap storage\nTrue True\nanother string kept on the heap\nTrue\n"
    );
}

#[test]
fn predicate_misuse_names_the_available_pair() {
    let message = rejected("r: Result[i64, str] = Ok(1)\nprint(r.is_some()).unwrap()\n");
    assert!(
        message.contains(
            "is_some requires an Option, got Result[i64, str]; a Result provides `is_ok` and `is_err`"
        ),
        "{message}"
    );
    let message = rejected("o: Option[i64] = Some(1)\nprint(o.is_err()).unwrap()\n");
    assert!(
        message.contains(
            "is_err requires a Result, got Option[i64]; an Option provides `is_some` and `is_none`"
        ),
        "{message}"
    );
    let message = rejected("o: Option[i64] = Some(1)\nprint(o.is_some(1)).unwrap()\n");
    assert!(message.contains("is_some takes no arguments"), "{message}");
    let message = rejected("x = 1\nprint(x.is_ok()).unwrap()\n");
    assert!(message.contains("unsupported method `is_ok`"), "{message}");
}

#[test]
fn classes_keep_their_own_methods_with_these_names() {
    assert_eq!(
        output(
            r#"
class Probe:
    value: i64

    def is_ok(self: &Probe) -> i64:
        self.value

    def rotate_left(self: &Probe, by: str) -> str:
        by

    def byte_len(self: &Probe) -> str:
        return "mine"

def main() -> ():
    probe = Probe(7)
    print(probe.is_ok(), probe.rotate_left("x"), probe.byte_len()).unwrap()
"#
        ),
        "7 x mine\n"
    );
}

#[test]
fn rotation_matches_rust_at_every_width() {
    fn expected<T>(
        value: T,
        left: fn(T, u32) -> T,
        right: fn(T, u32) -> T,
        counts: &[u32],
    ) -> String
    where
        T: Copy + std::fmt::Display,
    {
        counts
            .iter()
            .map(|&count| format!("{} {}\n", left(value, count), right(value, count)))
            .collect()
    }
    let counts = |width: u32| [0, 1, 3, width - 1, width, width + 1];
    let program = |ty: &str, value: &str, width: u32| {
        let mut source = format!("x: {ty} = {value}\n");
        for count in counts(width) {
            source += &format!(
                "print(x.rotate_left({count}{ty}), x.rotate_right({count}{ty})).unwrap()\n"
            );
        }
        output(&source)
    };
    assert_eq!(
        program("u8", "129", 8),
        expected(129u8, u8::rotate_left, u8::rotate_right, &counts(8))
    );
    assert_eq!(
        program("i8", "-2", 8),
        expected(-2i8, i8::rotate_left, i8::rotate_right, &counts(8))
    );
    assert_eq!(
        program("u16", "4660", 16),
        expected(4660u16, u16::rotate_left, u16::rotate_right, &counts(16))
    );
    assert_eq!(
        program("i16", "-4660", 16),
        expected(-4660i16, i16::rotate_left, i16::rotate_right, &counts(16))
    );
    assert_eq!(
        program("u32", "305419896", 32),
        expected(
            305419896u32,
            u32::rotate_left,
            u32::rotate_right,
            &counts(32)
        )
    );
    assert_eq!(
        program("i32", "-305419896", 32),
        expected(
            -305419896i32,
            i32::rotate_left,
            i32::rotate_right,
            &counts(32)
        )
    );
    assert_eq!(
        program("u64", "81985529216486895", 64),
        expected(
            81985529216486895u64,
            u64::rotate_left,
            u64::rotate_right,
            &counts(64)
        )
    );
    assert_eq!(
        program("i64", "-81985529216486895", 64),
        expected(
            -81985529216486895i64,
            i64::rotate_left,
            i64::rotate_right,
            &counts(64)
        )
    );
}

#[test]
fn rotation_counts_wrap_and_receivers_stay_readable() {
    assert_eq!(
        output(
            r#"
mut n = -2i8
print(n.rotate_left(-1), n.rotate_right(-1)).unwrap()
print(n.rotate_left(1).rotate_right(1)).unwrap()
n = 1
print((n + 1).rotate_left(2)).unwrap()
wide = 1u64
print(wide.rotate_left(200), wide.rotate_right(1)).unwrap()
"#
        ),
        "127 -3\n-2\n8\n256 9223372036854775808\n"
    );
}

#[test]
fn rotation_requires_one_count_of_the_receiver_type() {
    let message = rejected("print(1u8.rotate_left(1u16)).unwrap()\n");
    assert!(message.contains("expected u8, got u16"), "{message}");
    let message = rejected("print(1u8.rotate_right()).unwrap()\n");
    assert!(
        message.contains("rotate_right takes one count of the receiver's type"),
        "{message}"
    );
    let message = rejected("x = 1.5\nprint(x.rotate_left(1)).unwrap()\n");
    assert!(
        message.contains("unsupported method `rotate_left`"),
        "{message}"
    );
}

#[test]
fn byte_len_counts_utf8_bytes() {
    assert_eq!(
        output(
            r#"
def measure(text: &str) -> i64:
    text.byte_len()

def main() -> ():
    word = "héllo"
    print(len(word), word.byte_len()).unwrap()
    print("".byte_len(), "a\0b".byte_len(), "日本語".byte_len(), "😀".byte_len()).unwrap()
    joined = word.concat(" wörld, beyond inline storage").unwrap()
    print(len(joined), joined.byte_len(), measure(&joined)).unwrap()
    print(word.concat("!").unwrap().byte_len(), joined).unwrap()
"#
        ),
        "5 6\n0 3 9 4\n34 36 36\n7 héllo wörld, beyond inline storage\n"
    );
}

#[test]
fn byte_len_diagnostics() {
    let message = rejected("print(\"a\".byte_len(1)).unwrap()\n");
    assert!(message.contains("byte_len takes no arguments"), "{message}");
    let message = rejected("print(\"a\".len()).unwrap()\n");
    assert!(
        message.contains(
            "str has no `len` method; `len(text)` counts scalars and `text.byte_len()` counts UTF-8 bytes"
        ),
        "{message}"
    );
    let message = rejected("xs = [1].unwrap()\nprint(xs.byte_len()).unwrap()\n");
    assert!(
        message.contains("unsupported method `byte_len`"),
        "{message}"
    );
}
