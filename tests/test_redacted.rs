//! `@redacted` classes and enums (issue #586) format as `TypeName(???)` in
//! every automatic formatting path, at any nesting depth: `print`, `str.repr`,
//! f-string fields with any conversion or spec, and `main`'s error report.
//! Field access, equality, hashing and ordering are unchanged.
mod support;
use support::assert_stdout as expect;

const TYPES: &str = r#"import std.fmt

@redacted
class Key:
    secret: str
    id: i64

@redacted
class Password:
    value: str

@newtype
class Wrapped:
    key: Key

class Holder[T]:
    item: T

@redacted
enum Token:
    Bearer(str)
    Basic(str, str)

@redacted
@copy
class Pin:
    digits: i64

enum Event:
    Login(Pin)
    Logout

class Config:
    name: str
    key: Key
    password: Password

type Fault = AllocError | IoError | std.fmt.FormatError
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!("{TYPES}\ndef main() -> Result[(), Fault]:\n{body}    Ok(())\n")
}

#[test]
fn direct_and_nested_values_print_redacted() {
    expect(
        &program(
            r#"
k = Key("hunter2", 7)
print(k)?
print(Password("pw"))?
print(Token.Basic("user", "pw"))?
print("a", Password("pw"), 3)?
print(Config("app", Key("s", 1), Password("p")))?
print([Key("a", 1), Key("b", 2)]?)?
print({"k": Password("pw")}?)?
print((Key("t", 2), 3))?
o: Option[Key] = Some(Key("x", 1))
print(o)?
r: Result[Key, str] = Ok(Key("x", 1))
print(str.repr(r)?)?
print(Event.Login(Pin(1234)))?
print(Holder(Key("s", 1)))?
print(Wrapped(Key("s", 1)))?
"#,
        ),
        "Key(???)\n\
         Password(???)\n\
         Token(???)\n\
         a Password(???) 3\n\
         Config(name=\"app\", key=Key(???), password=Password(???))\n\
         [Key(???), Key(???)]\n\
         {\"k\": Password(???)}\n\
         (Key(???), 3)\n\
         Option[Key].Some(Key(???))\n\
         Result[Key, str].Ok(Key(???))\n\
         Event.Login(Pin(???))\n\
         Holder[Key](item=Key(???))\n\
         Wrapped(key=Key(???))\n",
    );
}

#[test]
fn repr_and_format_strings_have_no_escape() {
    expect(
        &program(
            r#"
k = Key("hunter2", 7)
print(str.repr(k)?)?
print(str.repr(Password("pw"))?)?
print(f"{k}|{k!r}|{k!s}|{k!a}|{k:>10}|{Password('z')=}"?)?
"#,
        ),
        "Key(???)\n\
         Password(???)\n\
         Key(???)|Key(???)|Key(???)|Key(???)|  Key(???)|Password('z')=Password(???)\n",
    );
}

#[test]
fn generic_redacted_types_name_their_arguments() {
    expect(
        "@redacted\nclass Secret[T]:\n    value: T\n\n\
         @redacted\nenum Choice[T]:\n    One(T)\n\n\
         def main() -> Result[(), AllocError | IoError]:\n    \
         print(Secret(5), Choice[str].One(\"x\"))?\n    Ok(())\n",
        "Secret[i64](???) Choice[str](???)\n",
    );
}

#[test]
fn fields_equality_and_copies_are_unchanged() {
    expect(
        &program(
            r#"
k = Key("hunter2", 7)
print(k.secret, k.id)?
print(k == Key("hunter2", 7), k == Key("other", 7))?
p = Pin(4)
q = p
print(p, q, p == q)?
"#,
        ),
        "hunter2 7\nTrue False\nPin(???) Pin(???) True\n",
    );
}

#[test]
fn an_explicit_display_method_still_formats_f_strings() {
    // Author-written `display` is explicit formatting, like a std.report
    // Formattable implementation; only automatic formatting is redacted.
    expect(
        "from std.fmt import Formatter, FormatError\n\n\
         @redacted\npub class Shown:\n    value: str\n    \
         pub def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]:\n        \
         out.text(\"shown\")\n\n\
         def main() -> Result[(), AllocError | IoError | FormatError]:\n    \
         s = Shown(\"x\")\n    print(f\"{s}\"?, s)?\n    Ok(())\n",
        "shown Shown(???)\n",
    );
}

#[test]
fn main_error_report_is_redacted() {
    let output = support::run(
        "@redacted\nclass Key:\n    secret: str\n\n\
         def main() -> Result[(), Key]:\n    Err(Key(\"hunter2\"))\n",
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Key(???)"), "{stderr}");
    assert!(!stderr.contains("hunter2"), "{stderr}");
}

#[test]
fn misplaced_redacted_annotations_are_diagnosed() {
    for (source, message) in [
        (
            "@redacted\ndef f() -> ():\n    pass",
            "`@redacted` applies only to class and enum declarations",
        ),
        (
            "@redacted\nprotocol P:\n    def f(self: &Self) -> ()",
            "`@redacted` applies only to class and enum declarations",
        ),
        (
            "@redacted\ntype Alias = i64",
            "`@redacted` applies only to class and enum declarations",
        ),
        (
            "class C:\n    @redacted\n    def f(self: &C) -> ():\n        pass",
            "`@redacted` applies only to class and enum declarations, not class members",
        ),
        ("@redacted\n@redacted\nclass D:\n    x: i64", "duplicate"),
        (
            "@redacted(True)\nclass D:\n    x: i64",
            "takes no arguments",
        ),
        (
            "@copy\nenum Color:\n    Red",
            "`@copy` applies only to class declarations",
        ),
    ] {
        let error = support::check_source(source).unwrap_err().to_string();
        assert!(error.contains(message), "{source}\n{error}");
    }
}

/// Redaction writes a fixed marker into the same checked buffer as ordinary
/// printing: it needs no more allocations than printing the same fields
/// unredacted, and a refused allocation is reported as an error.
#[cfg(feature = "runtime-checks")]
#[test]
fn redacted_printing_allocates_no_more_than_ordinary_printing() {
    support::assert_visible_stdout(
        r#"import std.fmt

@redacted
class Key:
    secret: str
    id: i64

class Plain:
    secret: str
    id: i64

type Fault = AllocError | IoError | std.fmt.FormatError

# The fewest allocations under which `print(value)` succeeds.
def needed[T](value: &T) -> Result[i64, Fault]:
    for n in range(0, 16):
        marker = f"__test_fail_allocations_after_{n}__"?
        print(marker)?
        result = print(value)
        print("__test_restore_allocations__")?
        if result.is_ok():
            return Ok(n)
    Ok(-1)

def main() -> Result[(), Fault]:
    redacted = needed(&Key("hunter2", 7))?
    plain = needed(&Plain("hunter2", 7))?
    print(redacted > 0, redacted <= plain)?
    Ok(())
"#,
        "Key(???)\nPlain(secret=\"hunter2\", id=7)\nTrue True",
    );
}
