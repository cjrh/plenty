//! A class with one field and no `__del__` is represented as that field
//! (issue #547): same slot, same bytes, no record. Construction, field access
//! and moving the field out cost nothing, and the class still formats, copies,
//! compares, releases and crosses C as the class.
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

#[test]
fn newtypes_behave_as_their_class() {
    native(
        r#"
class Wrap:
    x: i64

@copy
class Meters:
    value: f64

class Name:
    text: str

class Maybe:
    inner: Option[str]

class Span:
    items: range

class Pair:
    a: i64
    b: str

class Outer:
    pair: Pair

class Twice:
    w: Wrap

class Budget:
    remaining: i64

    def spend(self: &mut Budget) -> ():
        self.remaining -= 1

class Boxed[T]:
    item: T

def take(w: Wrap) -> i64:
    w.x

def main() -> Result[(), AllocError | IoError]:
    w = Wrap(1)
    print(w)?
    print(take(w))?
    m = Meters(2.5)
    n = m
    print(m, n, m == n)?
    name = Name("hi")
    print(name, name.text)?
    print(Maybe(Some("there")))?
    span = Span(range(3))
    mut total = 0
    for i in span.items:
        total += i
    print(total)?
    outer = Outer(Pair(7, "seven"))
    print(outer, outer.pair.b)?
    t = Twice(Wrap(9))
    print(t, t.w.x)?
    mut budget = Budget(3)
    budget.spend()
    budget.spend()
    print(budget.remaining)?
    xs = [Wrap(1), Wrap(2)]?
    print(xs, Wrap(1) == Wrap(1), Wrap(1) == Wrap(2))?
    opt: Option[Wrap] = Some(Wrap(5))
    match opt:
        case Some(v):
            print(v.x)?
        case Nothing:
            print("none")?
    b = Boxed("generic")
    print(b, b.item)?
    names = [Name("a"), Name("b")]?
    for nm in &names:
        print(nm.text)?
    moved = Name("moved").text
    print(moved)?
    Ok(())
"#,
        "Wrap(x=1)\n1\nMeters(value=2.5) Meters(value=2.5) True\nName(text=\"hi\") hi\n\
         Maybe(inner=Option[str].Some(\"there\"))\n3\n\
         Outer(pair=Pair(a=7, b=\"seven\")) seven\nTwice(w=Wrap(x=9)) 9\n1\n\
         [Wrap(x=1), Wrap(x=2)] True False\n5\nBoxed[str](item=\"generic\") generic\na\nb\nmoved\n",
    );
}

#[test]
fn destructors_run_once_through_newtype_wrappers() {
    // A one-field class with `__del__` keeps its record; a newtype around it
    // forwards ownership without a second cleanup.
    native(
        r#"
class Guarded:
    label: str

    def __del__(self) -> ():
        print("drop ", self.label).unwrap()

class Holder:
    guarded: Guarded

def main() -> Result[(), AllocError | IoError]:
    h = Holder(Guarded("inner"))
    print(h)?
    g = h.guarded
    print("moved out")?
    drop(g)
    print("after drop")?
    kept = Holder(Guarded("kept"))
    print(kept.guarded.label)?
    Ok(())
"#,
        "Holder(guarded=Guarded(label=\"inner\"))\nmoved out\ndrop  inner\nafter drop\nkept\ndrop  kept\n",
    );
}

#[test]
fn one_field_clist_rows_read_write_and_copy() {
    native(
        r#"
class Row:
    x: i32

def main() -> Result[(), AllocError | IoError]:
    mut rows = clist[Row]()
    for n in range(5):
        rows.append(Row(i32(n))).unwrap()
    rows[1].x = 10i32
    print(rows[1].x, rows.column(Row.x).filter(def(x: &i32) -> bool: *x > 2i32).count())?
    print(rows.copy_row(1), rows.copy_row(-1), rows.copy_row(5))?
    print(rows.remove(1), rows.len(), rows[1].x)?
    rows.clear()
    print(rows.len(), rows.remove(0))?
    Ok(())
"#,
        "10 3\nOption[Row].Some(Row(x=10)) Option[Row].Some(Row(x=4)) Option[Row].Nothing\n\
         Option[Row].Some(Row(x=10)) 4 2\n0 Option[Row].Nothing\n",
    );
}

/// The runtime operations called by the function whose dump section name
/// starts with `name`: the `N` of each `plenty_op_<N>` call.
fn dispatches(dir: &std::path::Path, name: &str) -> Vec<u32> {
    let text = std::fs::read_to_string(dir.join("out").join("asm.txt")).unwrap();
    let start = text
        .find(&format!("==== {name}"))
        .unwrap_or_else(|| panic!("{name} not dumped"));
    let rest = &text[start + 5..];
    let section = &rest[..rest.find("\n==== ").unwrap_or(rest.len())];
    section
        .lines()
        .filter_map(|line| line.rsplit_once("# plenty_op_"))
        .map(|(_, code)| code.parse().expect("operation number"))
        .collect()
}

#[test]
fn wrapping_a_file_stream_makes_no_runtime_calls() {
    // The issue #544 shape: constructing a newtype around an owned runtime
    // object and moving the field out is a register move, not a record copy.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.plenty"),
        r#"from std.io import FileStream

class Wrap:
    _file: File

    def out(self: Wrap) -> Outer:
        Outer(FileStream.new(self._file))

class Outer:
    _stream: FileStream

def cycle(file: File) -> Outer:
    Wrap(file).out()

def main() -> ():
    ()
"#,
    )
    .unwrap();
    let output = plenty(dir.path(), &["--dump", "asm", "main.plenty", "-o", "out"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(dispatches(dir.path(), "cycle"), Vec::<u32>::new());
    assert_eq!(dispatches(dir.path(), "Wrap.out"), Vec::<u32>::new());
    assert_eq!(dispatches(dir.path(), "Wrap.new"), Vec::<u32>::new());
}

#[test]
fn newtype_handles_cross_c_as_owned_borrowed_and_consumed_handles() {
    // A newtype has no record, but a C handle is still a box holding one slot:
    // create, borrow, mutate through a loan, consume, and destroy all use the
    // descriptor-driven paths that multi-field classes use.
    let source = r#"
class Label:
    text: str
def repeated(count: i64) -> Result[str, AllocError]:
    mut text = ""
    for _ in range(count):
        text = (text + "x")?
    Ok(text)
export def create(count: i64) -> Result[Label, AllocError] = "calc_create":
    Ok(Label(repeated(count)?))
export def length(a: &Label, b: &Label) -> i64 = "calc_length":
    len(a.text) + len(b.text)
export def rename(owner: &mut Label, count: i64) -> Result[(), AllocError] = "calc_rename":
    owner.text = repeated(count)?
    Ok(())
export def consume(owner: Label) -> i64 = "calc_consume":
    len(owner.text)
export def identity(owner: Label) -> Result[Label, AllocError] = "calc_identity":
    Ok(owner)
"#;
    for kind in [plenty::LibraryKind::Static, plenty::LibraryKind::Shared] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let path = root.join("source.plenty");
        std::fs::write(&path, source).unwrap();
        let library = root.join(if kind == plenty::LibraryKind::Static {
            "libcalc.a"
        } else {
            "libcalc.so"
        });
        let options = plenty::LibraryOptions::new("calc", kind);
        let artifacts = plenty::compile_file_to_library(&path, &library, None, &options).unwrap();
        let header = std::fs::read_to_string(&artifacts.header).unwrap();
        assert!(header.contains("const calc_Label * p0"), "{header}");
        let c = root.join("labels.c");
        std::fs::write(
            &c,
            r#"
#include "calc.h"
#include <assert.h>
int main(void) {
    calc_Label *owner = 0;
    uint32_t error = 99;
    assert(calc_create(4, &owner, &error) == 0);
    assert(calc_length(owner, owner) == 8);
    assert(calc_rename(owner, 2, &error) == 0);
    assert(calc_length(owner, owner) == 4);
    calc_Label *returned = 0;
    assert(calc_identity(owner, &returned, &error) == 0 && returned != 0);
    assert(calc_consume(returned) == 2);
    assert(calc_create(7, &owner, &error) == 0);
    calc_Label_destroy(owner);
    return 0;
}
"#,
        )
        .unwrap();
        let executable = root.join("caller");
        let args = std::fs::read_to_string(artifacts.link_args).unwrap();
        for compiler in ["cc", "c++"] {
            let output = std::process::Command::new(compiler)
                .args(["-Wall", "-Wextra", "-Werror"])
                .arg(&c)
                .arg(&library)
                .args(args.lines())
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let run = std::process::Command::new(&executable).output().unwrap();
            assert!(
                run.status.success(),
                "{}",
                String::from_utf8_lossy(&run.stderr)
            );
        }
        let app = root.join("main.plenty");
        std::fs::write(
            &app,
            "import calc\ndef main() -> Result[(), AllocError | IoError]:\n    mut owner = calc.create(4)?\n    print(calc.length(&owner, &owner))?\n    calc.rename(&mut owner, 2)?\n    print(calc.consume(calc.identity(owner)?))?\n    Ok(())\n",
        )
        .unwrap();
        let options = plenty::CompileOptions {
            link_args: vec![library.into_os_string()],
            ..Default::default()
        };
        plenty::compile_file_to_executable_with_options(&app, &executable, None, &options).unwrap();
        let run = std::process::Command::new(&executable).output().unwrap();
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(run.stdout, b"8\n2\n");
    }
}
