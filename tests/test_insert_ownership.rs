//! Inserting operations adopt their owned operands, and mutations through
//! `&mut` read the receiver in place (issue #762). A moved or copied key,
//! element or value is counted once on the stack, and that count becomes the
//! table's; a failed insert or a `get_or_insert` hit drops it exactly once.
//! Under `--all-features` the allocation-checks runtime also checks at exit
//! that every count was balanced.
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

// Each function stores a moved parameter through a `&mut` receiver, so the
// call retains neither.
const STORES: &str = r#"def add_moved(seen: &mut set[str], word: str) -> Result[(), AllocError]:
    seen.add(word)?
    Ok(())

def insert_moved(counts: &mut dict[str, i64], key: str) -> Result[(), AllocError]:
    counts.insert(key, 1)?
    Ok(())

def append_moved(names: &mut list[str], name: str) -> Result[(), AllocError]:
    names.append(name)?
    Ok(())

def tally(counts: &mut dict[str, i64], key: str) -> Result[(), AllocError | OverflowError[i64]]:
    *counts.get_or_insert(key, 0)? += 1
    Ok(())

def clear_all(counts: &mut dict[str, i64]) -> ():
    counts.clear()

def reorder(names: &mut list[str]) -> ():
    names.reverse()
    names.sort()
    names.swap(0, 1)

def narrow(seen: &mut set[str], counts: &dict[str, i64], dropped: &dict[str, i64]) -> ():
    seen.intersection_update(counts.keys())
    seen.difference_update(dropped.keys())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut seen = set[str]()?
    mut counts: dict[str, i64] = {}?
    mut names: list[str] = []?
    for word in ["a", "b", "a"]?:
        key = (word + "!")?
        add_moved(&mut seen, copy(&key)?)?
        insert_moved(&mut counts, copy(&key)?)?
        append_moved(&mut names, copy(&key)?)?
        tally(&mut counts, key)?
    print(len(seen), counts, names)?
    add_moved(&mut seen, "c!")?
    mut dropped: dict[str, i64] = {}?
    insert_moved(&mut dropped, "b!")?
    reorder(&mut names)
    narrow(&mut seen, &counts, &dropped)
    print(seen, names)?
    clear_all(&mut counts)
    print(len(counts))?
    print("__test_small_live_heap__")?
    Ok(())
"#;

#[test]
fn stores_through_mut_receivers_count_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), STORES).unwrap();
    let args = ["--release", "--dump", "asm", "p.plenty", "-o", "out"];
    let output = plenty(dir.path(), &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = std::fs::read_to_string(dir.path().join("out").join("asm.txt")).unwrap();
    for name in [
        "add_moved",
        "insert_moved",
        "append_moved",
        "tally",
        "clear_all",
        "reorder",
        "narrow",
    ] {
        let body = support::asm_body(&asm, name);
        assert!(!body.contains("plenty_retain"), "{name}:\n{body}");
        // A `get_or_insert` hit still releases the caller's key. `clear`,
        // `reverse`, `sort`, `swap` and the set updates return unit, not a
        // retained alias of the receiver (issue #813).
        let releases = body.matches("plenty_release").count();
        let expected = usize::from(name == "tally");
        assert_eq!(releases, expected, "{name}:\n{body}");
    }
    let output = plenty(dir.path(), &["--release", "p.plenty"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    let visible: Vec<_> = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect();
    assert_eq!(
        visible,
        [
            r#"2 {"a!": 2, "b!": 2} ["a!", "b!", "a!"]"#,
            r#"{"a!"} ["a!", "a!", "b!"]"#,
            "0"
        ]
    );
}

// Moved, copied and borrowed (read through a reference) operands, for local
// and `&mut` receivers, with keys that are new and present. The keys are
// longer than inline strings, so each is counted.
#[test]
fn moved_copied_and_borrowed_operands_are_stored_once() {
    native(
        r#"
def fill(counts: &mut dict[str, i64], seen: &mut set[str], names: &mut list[str], word: &str) -> Result[(), AllocError | OverflowError[i64]]:
    counts.insert(*word, 1)?
    seen.add(*word)?
    names.append(*word)?
    *counts.get_or_insert(*word, 0)? += 1
    Ok(())

def main() -> Result[(), OverflowError[i64]]:
    mut counts: dict[str, i64] = {}.unwrap()
    mut seen = set[str]().unwrap()
    mut names: list[str] = [].unwrap()
    mut values: dict[str, str] = {}.unwrap()
    for word in ["x", "y", "x"].unwrap():
        key = (word + "-counted-key").unwrap()
        value = (word + "-counted-value").unwrap()
        # Copied: both stay usable after the insert.
        counts.insert(key, 0).unwrap()
        seen.add(key).unwrap()
        names.append(key).unwrap()
        values.insert(key, value).unwrap()
        print(key, value).unwrap()
        # Borrowed: read through a reference.
        fill(&mut counts, &mut seen, &mut names, &key).unwrap()
        # Moved: the last reads.
        *counts.get_or_insert(key, 0).unwrap() += 1
        values.insert((word + "-moved-key").unwrap(), value).unwrap()
    print(counts).unwrap()
    print(len(seen), names).unwrap()
    print(values).unwrap()
    print(counts.pop("x-counted-key"), len(counts)).unwrap()
    names.reverse()
    seen.discard("y-counted-key")
    print(names[0], len(names), len(seen)).unwrap()
    print("__test_small_live_heap__").unwrap()
    Ok(())
"#,
        r#"x-counted-key x-counted-value
y-counted-key y-counted-value
x-counted-key x-counted-value
{"x-counted-key": 3, "y-counted-key": 3}
2 ["x-counted-key", "x-counted-key", "y-counted-key", "y-counted-key", "x-counted-key", "x-counted-key"]
{"x-counted-key": "x-counted-value", "x-moved-key": "x-counted-value", "y-counted-key": "y-counted-value", "y-moved-key": "y-counted-value"}
Option[i64].Some(3) 1
x-counted-key 6 1"#,
    );
}

/// A failed insert drops a moved operand once and leaves a copied one with
/// its source; a `get_or_insert` hit leaves the caller's copyable operands
/// to generated code, which releases them. Empty collections have no
/// storage, so each first insert must allocate.
#[cfg(feature = "runtime-checks")]
#[test]
fn failed_inserts_drop_owned_operands_once() {
    native(
        r#"
def main() -> ():
    mut counts: dict[str, str] = {}.unwrap()
    mut seen = set[str]().unwrap()
    mut names: list[str] = [].unwrap()
    mut present: dict[str, str] = {"present-key": "present-value"}.unwrap()
    key = ("moved-dict-key" + "").unwrap()
    moved = ("moved-list-item" + "").unwrap()
    copied = ("copied-operand" + "").unwrap()
    value = ("moved-dict-value" + "").unwrap()
    hit = ("present-key" + "").unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    match counts.insert(copied, copied):
        case Ok(_):
            print("unexpected success").unwrap()
        case Err(error):
            print("dict copied").unwrap()
    match seen.add(copied):
        case Ok(_):
            print("unexpected success").unwrap()
        case Err(error):
            print("set copied").unwrap()
    match names.append(moved):
        case Ok(_):
            print("unexpected success").unwrap()
        case Err(error):
            print("list moved").unwrap()
    match counts.get_or_insert(key, value):
        case Ok(stored):
            print("unexpected success").unwrap()
        case Err(error):
            print("get_or_insert moved").unwrap()
    match present.get_or_insert(hit, copied):
        case Ok(stored):
            print(*stored).unwrap()
        case Err(error):
            print("unexpected failure").unwrap()
    print("__test_restore_allocations__").unwrap()
    print(copied, len(counts), len(seen), len(names), len(present)).unwrap()
    print("__test_small_live_heap__").unwrap()
"#,
        "dict copied\nset copied\nlist moved\nget_or_insert moved\npresent-value\ncopied-operand 0 0 0 1",
    );
}

// Mutations through `&mut` receivers reached through a field, a list element
// and a `get_mut` row, which read the receiver in place, keep every count
// balanced, including destructors that run while values are replaced, popped
// and cleared.
#[test]
fn nested_mut_receivers_keep_counts_balanced() {
    native(
        r#"
class Noisy:
    name: str

    def __del__(self: &mut Noisy) -> ():
        print("drop", self.name).unwrap()

class Bag:
    items: list[str]
    index: dict[str, list[str]]
    tags: set[str]

    def put(self: &mut Bag, word: str) -> Result[(), AllocError]:
        self.items.append(copy(&word)?)?
        self.items.sort()
        self.items.swap(0, 0)
        self.tags.add(copy(&word)?)?
        match self.index.get_mut(&word):
            case Some(row):
                row.append(copy(&word)?)?
                row.extend([copy(&word)?]?)?
                row.reverse()
                row.pop()
            case Nothing:
                self.index.insert(copy(&word)?, [word]?)?
        Ok(())

def grow(grid: &mut list[set[str]], word: str) -> Result[(), AllocError]:
    grid[0].add(copy(&word)?)?
    grid[1].update(set[str]()?)?
    grid[1].add(word)?
    grid[0].intersection_update(&grid[1])
    grid[1].difference_update(&grid[0])
    Ok(())

def main() -> Result[(), AllocError | IoError]:
    mut bag = Bag([]?, {}?, set[str]()?)
    for word in ["alpha-counted-key", "beta-counted-key", "alpha-counted-key"]?:
        bag.put((word + "")?)?
    print(bag.items, bag.index, len(bag.tags))?
    mut grid: list[set[str]] = [set[str]()?, set[str]()?]?
    for word in ["one-counted-word", "two-counted-word"]?:
        grow(&mut grid, (word + "")?)?
    print(len(grid[0]), len(grid[1]))?
    mut owners: dict[str, Noisy] = {}?
    owners.insert("k", Noisy("first"))?
    owners.insert("k", Noisy("second"))?
    owners.pop("k")
    mut ns: list[Noisy] = [Noisy("a"), Noisy("b")]?
    ns.reverse()
    ns.clear()
    print("__test_small_live_heap__")?
    Ok(())
"#,
        r#"["alpha-counted-key", "alpha-counted-key", "beta-counted-key"] {"alpha-counted-key": ["alpha-counted-key", "alpha-counted-key"], "beta-counted-key": ["beta-counted-key"]} 2
1 0
drop first
drop second
drop b
drop a"#,
    );
}

/// A failed append through a `get_mut` row drops its moved element once.
#[cfg(feature = "runtime-checks")]
#[test]
fn failed_append_through_row_drops_element_once() {
    native(
        r#"
def put(d: &mut dict[str, list[str]], k: str, v: str) -> Result[(), AllocError]:
    match d.get_mut(&k):
        case Some(row):
            row.append(v)?
        case Nothing:
            d.insert(k, [v]?)?
    Ok(())

def main() -> ():
    mut d: dict[str, list[str]] = {}.unwrap()
    k = ("key-long-enough-to-count" + "").unwrap()
    v = ("value-long-enough-to-count" + "").unwrap()
    put(&mut d, copy(&k).unwrap(), copy(&v).unwrap()).unwrap()
    k2 = copy(&k).unwrap()
    v2 = copy(&v).unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    failed = put(&mut d, k2, v2).is_err()
    popped = d.pop(k).is_some()
    print("__test_restore_allocations__").unwrap()
    print(failed, popped, v, len(d)).unwrap()
    print("__test_small_live_heap__").unwrap()
"#,
        "True True value-long-enough-to-count 0",
    );
}

// Single-`for` comprehensions lower as `collect()` pipelines; nested ones and
// displays build through the general comprehension lowering. Each moves its
// counted element into the insert and reads the result in place (issue #826).
const BUILDS: &str = r#"def keys(words: &list[str]) -> Result[set[str], AllocError]:
    {copy(w)? for w in words}

def lengths(words: &list[str]) -> Result[dict[str, i64], AllocError]:
    {copy(w)?: len(w) for w in words}

def names(words: &list[str]) -> Result[list[str], AllocError]:
    [copy(w)? for w in words if len(w) > 3]

def nested_keys(rows: &list[list[str]]) -> Result[set[str], AllocError]:
    {copy(w)? for row in rows for w in row}

def nested_lengths(rows: &list[list[str]]) -> Result[dict[str, i64], AllocError]:
    {copy(w)?: len(w) for row in rows for w in row}

def nested_names(rows: &list[list[str]]) -> Result[list[str], AllocError]:
    [copy(w)? for row in rows for w in row]

def pair(a: &str, b: &str) -> Result[dict[str, str], AllocError]:
    {copy(a)?: copy(b)?, copy(b)?: copy(a)?}

def upper(w: &str) -> str:
    w.upper().unwrap()

def copied(w: &str) -> Result[str, AllocError]:
    copy(w)

def collected(words: &list[str]) -> Result[list[str], AllocError]:
    iter(words).map(upper).collect()

def tried(words: &list[str]) -> Result[list[str], AllocError]:
    iter(words).map(copied).try_collect()

def generated(n: i64) -> Generator[str]:
    for i in range(n):
        yield ("generated-counted-word" + "").unwrap()

def listed(n: i64) -> Result[list[str], AllocError]:
    list(generated(n))

def main() -> Result[(), AllocError | IoError]:
    words = [("alpha-counted-word" + "")?, ("beta-counted-word" + "")?, ("alpha-counted-word" + "")?]?
    rows = [copy(&words)?, [("gamma-counted-word" + "")?]?]?
    print(keys(&words)?, lengths(&words)?, names(&words)?)?
    print(nested_keys(&rows)?, nested_lengths(&rows)?, nested_names(&rows)?)?
    print(pair(&words[0], &words[1])?)?
    print(collected(&words)?, tried(&words)?, listed(2)?)?
    print("__test_small_live_heap__")?
    Ok(())
"#;

#[test]
fn comprehension_inserts_count_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p.plenty"), BUILDS).unwrap();
    let args = ["--release", "--dump", "asm", "p.plenty", "-o", "out"];
    let output = plenty(dir.path(), &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let asm = std::fs::read_to_string(dir.path().join("out").join("asm.txt")).unwrap();
    for name in [
        "keys",
        "lengths",
        "names",
        "nested_keys",
        "nested_lengths",
        "nested_names",
        "pair",
        "collected",
        "tried",
        "listed",
    ] {
        let body = support::asm_body(&asm, name);
        // Neither the element nor the collection is retained, and nothing is
        // released on the straight-line path from an insert to the next
        // branch; later releases are on failure and exit paths. The last
        // insert of the `pair` display falls through to the release of the
        // owner's one copy at scope exit.
        assert!(!body.contains("plenty_retain"), "{name}:\n{body}");
        let lines: Vec<&str> = body.lines().collect();
        let mut inserts: Vec<usize> = (0..lines.len())
            .filter(|&i| lines[i].ends_with("# plenty_op_29"))
            .collect();
        assert!(!inserts.is_empty(), "{name}:\n{body}");
        if name == "pair" {
            assert_eq!(inserts.len(), 2, "{name}:\n{body}");
            inserts.pop();
        }
        for i in inserts {
            let after = lines[i + 1..]
                .iter()
                .take_while(|line| !support::native_branch(line));
            for line in after {
                assert!(!line.contains("plenty_release"), "{name}:\n{body}");
            }
        }
    }
    let output = plenty(dir.path(), &["--release", "p.plenty"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    let visible: Vec<_> = stdout
        .lines()
        .filter(|line| !line.starts_with("__test_"))
        .collect();
    assert_eq!(
        visible,
        [
            concat!(
                r#"{"alpha-counted-word", "beta-counted-word"} "#,
                r#"{"alpha-counted-word": 18, "beta-counted-word": 17} "#,
                r#"["alpha-counted-word", "beta-counted-word", "alpha-counted-word"]"#
            ),
            concat!(
                r#"{"alpha-counted-word", "beta-counted-word", "gamma-counted-word"} "#,
                r#"{"alpha-counted-word": 18, "beta-counted-word": 17, "gamma-counted-word": 18} "#,
                r#"["alpha-counted-word", "beta-counted-word", "alpha-counted-word", "gamma-counted-word"]"#
            ),
            r#"{"alpha-counted-word": "beta-counted-word", "beta-counted-word": "alpha-counted-word"}"#,
            concat!(
                r#"["ALPHA-COUNTED-WORD", "BETA-COUNTED-WORD", "ALPHA-COUNTED-WORD"] "#,
                r#"["alpha-counted-word", "beta-counted-word", "alpha-counted-word"] "#,
                r#"["generated-counted-word", "generated-counted-word"]"#
            ),
        ]
    );
}

#[test]
fn displays_reuse_hidden_operand_locals() {
    // Each dict display needs a result and a pending local; its key and value
    // locals are shared with earlier displays, so 80 displays fit in the 256
    // slots (issue #826).
    let mut source = String::from(
        "def many(a: &str, b: &str) -> Result[i64, AllocError]:\n    mut d = {copy(a)?: copy(b)?}?\n",
    );
    for _ in 0..80 {
        source.push_str("    d = {copy(a)?: copy(b)?}?\n");
    }
    source.push_str("    Ok(len(d))\n");
    support::check_source(&source).unwrap();
}
