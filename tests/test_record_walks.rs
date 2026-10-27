//! Records the compiler knows at compile time skip the runtime's generic
//! field walks (issue #550): large record copies and clears stay inline, a
//! local moved straight into a call is passed in place, and a class's
//! descriptor records which strings it owns and whether its bytes move
//! without repair, so list rows release and relocate without visiting each
//! field. Under `--all-features` the allocation-checks runtime also checks at
//! exit that every string was released exactly once.
mod support;
use support::assert_visible_stdout as native;

fn plenty(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap()
}

/// The assembly of the function whose dump section name starts with `name`.
fn section(text: &str, name: &str) -> String {
    let start = text
        .find(&format!("==== {name}"))
        .unwrap_or_else(|| panic!("{name} not dumped"));
    let rest = &text[start + 5..];
    rest[..rest.find("\n==== ").unwrap_or(rest.len())].to_string()
}

const COPY_PAYLOAD: &str = "# plenty_op_117";

const PATHS: &str = r#"class Point:
    x: i64
    y: i64

class Segment:
    start: Point
    end: Point
    label: str

class Path:
    first: Segment
    second: Segment
    id: i64

def total(s: &Segment) -> i64:
    s.start.x + s.start.y + s.end.x + s.end.y

def path(n: i64) -> Result[Path, AllocError]:
    a = Segment(Point(n, n + 1), Point(n + 2, n + 3), ("a" + str.from(n)?)?)
    b = Segment(Point(-n, 0), Point(0, -n), "b")
    Ok(Path(a, b, n))

def keep(p: Path) -> Path:
    p

def weight(p: Path) -> i64:
    total(&p.first) + total(&p.second) + p.id

def relay(n: i64) -> Result[i64, AllocError]:
    p = path(n)?
    q = path(n + 1)?
    # `p` moves in place while `q` is still live, and `q` after another call.
    first = weight(p)
    Ok(first + weight(keep(q)))

def main() -> Result[(), AllocError | IoError]:
    mut rows: list[Path] = []?
    for i in range(5):
        rows.append(keep(path(i)?))?
    mut sum = 0
    for r in &rows:
        sum += total(&r.first) + total(&r.second)
    print(sum, relay(3)?)?
    last = rows.pop()
    match last:
        case Some(p):
            print(p.first.label, p.id)?
        case Nothing:
            pass
    rows[0] = path(9)?
    print(rows[0].first.label, len(rows))?
    mut maybe: list[Option[Path]] = [Some(path(2)?), Nothing]?
    maybe.append(Some(path(3)?))?
    mut labels: dict[str, Segment] = {}?
    for n in [4, 5, 6]?:
        p = path(n)?
        key = "x" if n != 5 else "y"
        labels.insert(key, p.first)?
    print(labels["x"].label, len(maybe))?
    Ok(())
"#;

#[test]
fn string_records_release_and_relocate_once() {
    native(PATHS, "50 33\na4 4\na9 4\na6 3\n");
}

/// Copies of records larger than Cranelift's 32-byte inline limit used to
/// call `memmove`, and clears `memset`; a `Path` record is 176 bytes.
#[test]
fn large_record_copies_stay_inline() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), PATHS).unwrap();
    let args = ["--release", "--dump", "asm", "p.plenty", "-o", "out"];
    let output = plenty(dir.path(), &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(dir.path().join("out").join("asm.txt")).unwrap();
    for name in [
        "path ",
        "keep ",
        "weight ",
        "relay ",
        "Path.new ",
        "Segment.new ",
    ] {
        let asm = section(&text, name);
        // Cranelift loads libcall addresses, which the dump names `%Memmove`.
        for call in ["%Memmove", "%Memcpy", "%Memset", COPY_PAYLOAD] {
            assert!(!asm.contains(call), "{name} calls {call}:\n{asm}");
        }
    }
}

/// A class with `__del__` keeps the field walk: its destructor runs before
/// its fields are released, in field order, and a string-only sibling record
/// in the same row is released without it.
const ORDERED: &str = r#"class Res:
    name: str
    def __del__(self) -> ():
        print("drop", self.name).unwrap()

class Tag:
    text: str
    other: str

class Row:
    tag: Tag
    a: Res
    b: Res
    after: Tag

def row(n: str) -> Result[Row, AllocError]:
    Ok(Row(Tag((n + "t")?, "u"), Res((n + "a")?), Res((n + "b")?), Tag("v", (n + "w")?)))

def main() -> Result[(), AllocError | IoError]:
    mut rows: list[Row] = []?
    rows.append(row("x")?)?
    rows.append(row("y")?)?
    print(rows[1].tag.text, rows[0].after.other)?
    Ok(())
"#;

#[test]
fn destructor_records_keep_field_order() {
    native(ORDERED, "yt xw\ndrop xa\ndrop xb\ndrop ya\ndrop yb\n");
}
