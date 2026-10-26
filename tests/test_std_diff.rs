//! `std.diff`: edit operations over sequences, bytes, and text tokens, and
//! unified-diff output (issue #469).

const HEADER: &str = "import std.bytes\nimport std.diff\nfrom std.diff import Algorithm, Kind, Op, TextDiff\n";

const DECLARATIONS: &str = r#"
def code(op: &Op) -> str:
    match op.kind:
        case Kind.Equal:
            "="
        case Kind.Delete:
            "-"
        case Kind.Insert:
            "+"

def show(ops: &list[Op]) -> Result[(), AllocError | IoError]:
    for op in ops:
        print(code(op), op.old_start, op.old_end, op.new_start, op.new_end)?
    print("end")?
    Ok(())
"#;

fn program(body: &str) -> String {
    let body = body
        .trim_matches('\n')
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "{HEADER}{DECLARATIONS}\ndef main() -> Result[(), AllocError | IoError | std.bytes.BytesError]:\n{body}    Ok(())\n"
    )
}

fn execute(body: &str) -> std::process::Output {
    let source = program(body);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    let executable = dir.path().join("program");
    std::fs::write(&path, &source).unwrap();
    plenty::compile_file_to_executable(&path, &executable, None)
        .unwrap_or_else(|error| panic!("{source}\n{error}"));
    std::process::Command::new(executable).output().unwrap()
}

fn output(body: &str) -> String {
    let output = execute(body);
    assert!(
        output.status.success(),
        "{body}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn native(body: &str, expected: &str) {
    assert_eq!(output(body), expected.trim_matches('\n'), "{body}");
}

/// A Plenty string literal for `text`.
fn literal(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Equal,
    Delete,
    Insert,
}

#[derive(Debug)]
struct Op {
    kind: Kind,
    old: (usize, usize),
    new: (usize, usize),
}

/// The `show` output: one script per `end` line.
fn scripts(text: &str) -> Vec<Vec<Op>> {
    let mut all = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        if line == "end" {
            all.push(std::mem::take(&mut current));
            continue;
        }
        let fields: Vec<_> = line.split(' ').collect();
        let number = |index: usize| fields[index].parse::<usize>().unwrap();
        let kind = match fields[0] {
            "=" => Kind::Equal,
            "-" => Kind::Delete,
            "+" => Kind::Insert,
            other => panic!("unknown op {other}"),
        };
        current.push(Op {
            kind,
            old: (number(1), number(2)),
            new: (number(3), number(4)),
        });
    }
    all
}

/// Checks that `ops` edit `old` into `new` and returns the number of equal
/// elements. Operations are contiguous and nonempty, equal runs are merged,
/// and a deletion precedes its adjacent insertion.
fn validate<T: PartialEq + std::fmt::Debug>(old: &[T], new: &[T], ops: &[Op]) -> usize {
    let (mut o, mut n, mut equal) = (0, 0, 0);
    let mut previous: Option<Kind> = None;
    for op in ops {
        assert_eq!((op.old.0, op.new.0), (o, n), "{ops:?}");
        let (old_len, new_len) = (op.old.1 - op.old.0, op.new.1 - op.new.0);
        match op.kind {
            Kind::Equal => {
                assert!(old_len > 0 && old_len == new_len, "{ops:?}");
                assert_eq!(old[op.old.0..op.old.1], new[op.new.0..op.new.1]);
                equal += old_len;
            }
            Kind::Delete => {
                assert!(old_len > 0 && new_len == 0, "{ops:?}");
                assert_ne!(previous, Some(Kind::Insert), "{ops:?}");
            }
            Kind::Insert => assert!(new_len > 0 && old_len == 0, "{ops:?}"),
        }
        assert_ne!(previous, Some(op.kind), "{ops:?}");
        previous = Some(op.kind);
        (o, n) = (op.old.1, op.new.1);
    }
    assert_eq!((o, n), (old.len(), new.len()), "{ops:?}");
    equal
}

fn lcs<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut row = vec![0; b.len() + 1];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) % bound
    }

    fn text(&mut self, alphabet: &[char], max: u64) -> String {
        let size = self.below(max + 1);
        (0..size)
            .map(|_| alphabet[self.below(alphabet.len() as u64) as usize])
            .collect()
    }
}

#[test]
fn myers_finds_a_shortest_script_and_patience_a_valid_one() {
    let mut rng = Lcg(469);
    let mut pairs = vec![
        (String::new(), String::new()),
        (String::new(), "abc".to_owned()),
        ("abc".to_owned(), String::new()),
        ("abcabba".to_owned(), "cbabac".to_owned()),
    ];
    for round in 0..160 {
        let alphabet: &[char] = if round % 2 == 0 {
            &['a', 'b', 'c']
        } else {
            &['a', 'b', 'c', 'd', 'e', 'f', 'g']
        };
        let old = rng.text(alphabet, 24);
        // Derive the new text from the old one half the time, so long common
        // runs and isolated edits both occur.
        let new = if round % 4 < 2 {
            let mut chars: Vec<char> = old.chars().collect();
            for _ in 0..rng.below(4) {
                let at = rng.below(chars.len() as u64 + 1) as usize;
                match rng.below(3) {
                    0 if at < chars.len() => {
                        chars.remove(at);
                    }
                    1 if at < chars.len() => chars[at] = 'z',
                    _ => chars.insert(at, 'y'),
                }
            }
            chars.into_iter().collect()
        } else {
            rng.text(alphabet, 24)
        };
        pairs.push((old, new));
    }
    let cases = pairs
        .iter()
        .map(|(old, new)| format!("({}, {})", literal(old), literal(new)))
        .collect::<Vec<_>>()
        .join(", ");
    let text = output(&format!(
        r#"
for old, new in [{cases}]?:
    myers = std.diff.chars(old, new)?
    show(myers.ops())?
    patience = std.diff.chars(old, new, Algorithm.Patience)?
    show(patience.ops())?
"#
    ));
    let scripts = scripts(&text);
    assert_eq!(scripts.len(), 2 * pairs.len());
    for (index, (old, new)) in pairs.iter().enumerate() {
        let a: Vec<char> = old.chars().collect();
        let b: Vec<char> = new.chars().collect();
        let shortest = validate(&a, &b, &scripts[2 * index]);
        assert_eq!(shortest, lcs(&a, &b), "Myers on {old:?} -> {new:?}");
        validate(&a, &b, &scripts[2 * index + 1]);
    }
}

#[test]
fn patience_anchors_on_lines_unique_to_both_sides() {
    // When block `a` and block `b` swap places, Myers keeps the `a` lines and
    // the final brace; patience anchors on the unique `b` and `y` lines and
    // moves block `a` as whole lines.
    native(
        r#"
old = ["f", "{", "}", "g", "{", "}"]?
new = ["h", "{", "}", "g", "{", "}"]?
show(&std.diff.diff(&old, &new)?)?
moved_old = ["a", "{", "x", "}", "b", "{", "y", "}"]?
moved_new = ["b", "{", "y", "}", "a", "{", "x", "}"]?
show(&std.diff.diff(&moved_old, &moved_new)?)?
show(&std.diff.diff(&moved_old, &moved_new, Algorithm.Patience)?)?
"#,
        r#"
- 0 1 0 0
+ 1 1 0 1
= 1 6 1 6
end
+ 0 0 0 4
= 0 3 4 7
- 3 7 7 7
= 7 8 7 8
end
+ 0 0 0 4
= 0 4 4 8
- 4 8 8 8
end
"#,
    );
}

#[test]
fn generic_and_byte_diffs_compare_elements() {
    native(
        r#"
show(&std.diff.diff(&[1, 2, 3, 4]?, &[2, 3, 5, 4]?)?)?
empty: list[i64] = []?
show(&std.diff.diff(&empty, &empty)?)?
show(&std.diff.diff_bytes(&std.bytes.from_text("abc")?, &std.bytes.from_text("abd")?)?)?
ops = std.diff.diff(&["x", "y"]?, &["x", "z"]?)?
print(ops[0].is_equal(), ops[1].old_len(), ops[2].new_len())?
"#,
        r#"
- 0 1 0 0
= 1 3 0 2
+ 3 3 2 3
= 3 4 3 4
end
end
= 0 2 0 2
- 2 3 2 2
+ 3 3 2 3
end
True 1 1
"#,
    );
}

#[test]
fn text_tokens_concatenate_to_the_original_text() {
    native(
        r#"
d = std.diff.lines("one\r\ntwo\n\nlast", "one\r\n")?
print(d.old_tokens(), d.new_tokens(), d.has_changes())?
w = std.diff.words("  héllo\twide  world ", "héllo world")?
print(w.old_tokens(), w.new_tokens())?
c = std.diff.chars("añb", "ab")?
print(c.old_tokens())?
show(c.ops())?
same = std.diff.words("a b", "a b")?
print(same.has_changes(), len(same.ops()))?
"#,
        r#"
["one\r\n", "two\n", "\n", "last"] ["one\r\n"] True
["  ", "héllo", "\t", "wide", "  ", "world", " "] ["héllo", " ", "world"]
["a", "ñ", "b"]
= 0 1 0 1
- 1 2 1 1
= 2 3 1 2
end
False 1
"#,
    );
}

#[test]
fn unified_output_matches_diff_u() {
    // Each expected text is what GNU `diff -U<context>` prints for the pair.
    native(
        r#"
old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n"
new = "a\nb\nC\nd\ne\nf\ng\nh\ni\nj\nk"
print(std.diff.lines(old, new)?.unified(2, "old.txt", "new.txt")?)?
print(std.diff.lines(old, new)?.unified(4, "old.txt", "new.txt")?)?
print(std.diff.lines("", "x\n")?.unified(3, "a", "b")?)?
print(std.diff.lines("x\ny\n", "")?.unified(3, "a", "b")?)?
print(std.diff.lines("a\nb\nc\n", "a\nB\nc\n")?.unified(0, "a", "b")?)?
print(std.diff.lines("a\nb\nc\n", "a\nB\nc\n")?.unified(-4, "a", "b")?)?
print(std.diff.lines("a\nb", "a\nb\n")?.unified(1, "a", "b")?)?
print(str.repr(std.diff.lines("same\n", "same\n")?.unified(3, "a", "b")?)?)?
"#,
        r#"
--- old.txt
+++ new.txt
@@ -1,5 +1,5 @@
 a
 b
-c
+C
 d
 e
@@ -9,2 +9,3 @@
 i
 j
+k
\ No newline at end of file

--- old.txt
+++ new.txt
@@ -1,10 +1,11 @@
 a
 b
-c
+C
 d
 e
 f
 g
 h
 i
 j
+k
\ No newline at end of file

--- a
+++ b
@@ -0,0 +1 @@
+x

--- a
+++ b
@@ -1,2 +0,0 @@
-x
-y

--- a
+++ b
@@ -2 +2 @@
-b
+B

--- a
+++ b
@@ -2 +2 @@
-b
+B

--- a
+++ b
@@ -1,2 +1,2 @@
 a
-b
\ No newline at end of file
+b

""
"#,
    );
}

/// Applies a unified diff to `old`, checking every context and removed line.
fn apply(old: &str, patch: &str) -> String {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let mut out = String::new();
    let mut at = 0;
    let mut lines = patch.split_inclusive('\n').skip(2).peekable();
    while let Some(header) = lines.next() {
        let header = header.strip_prefix("@@ -").unwrap();
        let old_range = header.split(' ').next().unwrap();
        let (start, count) = match old_range.split_once(',') {
            Some((start, count)) => (start.parse::<usize>().unwrap(), count.parse().unwrap()),
            None => (old_range.parse::<usize>().unwrap(), 1),
        };
        let begin = if count == 0 { start } else { start - 1 };
        assert!(begin >= at, "{patch}");
        old_lines[at..begin].iter().for_each(|line| out.push_str(line));
        at = begin;
        while let Some(line) = lines.next_if(|line| !line.starts_with("@@")) {
            let mut body = line[1..].to_owned();
            if lines.next_if(|next| next.starts_with('\\')).is_some() {
                body.pop();
            }
            match &line[..1] {
                " " | "-" => {
                    assert_eq!(old_lines[at], body, "{patch}");
                    at += 1;
                    if line.starts_with(' ') {
                        out.push_str(&body);
                    }
                }
                "+" => out.push_str(&body),
                other => panic!("unexpected line {other:?} in {patch}"),
            }
        }
    }
    old_lines[at..].iter().for_each(|line| out.push_str(line));
    out
}

#[test]
fn unified_hunks_apply_to_reproduce_the_new_text() {
    let mut rng = Lcg(2026);
    let mut pairs = Vec::new();
    for _ in 0..80 {
        let line = |rng: &mut Lcg| ["a\n", "b\n", "c\n", "d\n", "e\n"][rng.below(5) as usize];
        let mut old: String = (0..rng.below(30)).map(|_| line(&mut rng)).collect();
        let mut new = String::new();
        for kept in old.split_inclusive('\n') {
            match rng.below(8) {
                0 => {}
                1 => new.push_str(line(&mut rng)),
                2 => {
                    new.push_str(kept);
                    new.push_str(line(&mut rng));
                }
                _ => new.push_str(kept),
            }
        }
        if rng.below(4) == 0 {
            old.push('z');
        }
        if rng.below(4) == 0 {
            new.push('z');
        }
        pairs.push((old, new, rng.below(4)));
    }
    let cases = pairs
        .iter()
        .map(|(old, new, context)| format!("({}, {}, {context})", literal(old), literal(new)))
        .collect::<Vec<_>>()
        .join(", ");
    let text = output(&format!(
        r#"
for old, new, context in [{cases}]?:
    print(std.diff.lines(old, new)?.unified(context, "a", "b")?, "=====")?
    print(std.diff.lines(old, new, Algorithm.Patience)?.unified(context, "a", "b")?, "=====")?
"#
    ));
    let patches: Vec<&str> = text.split(" =====").collect();
    assert_eq!(patches.len(), 2 * pairs.len() + 1);
    for (index, (old, new, context)) in pairs.iter().enumerate() {
        for patch in &patches[2 * index..2 * index + 2] {
            let patch = patch.strip_prefix('\n').unwrap_or(patch);
            if old == new {
                assert_eq!(patch, "");
                continue;
            }
            assert!(patch.starts_with("--- a\n+++ b\n"), "{patch}");
            assert_eq!(apply(old, patch), *new, "context {context}\n{patch}");
        }
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn diffing_reports_every_allocation_failure_as_alloc_error() {
    // Fail the n-th allocation for growing n: each attempt either reports
    // AllocError or, once n passes the last allocation, succeeds. None traps.
    native(
        r#"
mut failures = 0
mut after = 0
mut done = False
while not done:
    marker = (("__test_fail_allocations_after_" + str.from(after)?)? + "__")?
    print(marker)?
    result = std.diff.lines("a\nb\nlong line here\nc\n", "a\nlong line here\nc\nd\n", Algorithm.Patience)
    mut succeeded = False
    match result:
        case Ok(diff):
            match diff.unified(1, "old", "new"):
                case Ok(_):
                    succeeded = True
                case Err(_):
                    pass
        case Err(_):
            pass
    print("__test_restore_allocations__")?
    if succeeded:
        done = True
    else:
        failures += 1
    after += 1
print(failures > 10, failures == after - 1)?
"#,
        "True True",
    );
}
