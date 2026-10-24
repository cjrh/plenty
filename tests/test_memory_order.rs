//! Memory accesses through references keep their order in release builds.
//! Generated loads and stores set no Cranelift alias region (issue #440): a
//! reference may name a local, a list row, a class field or an inline-sum
//! payload, so a region that separates any two of them can forward a stale
//! load past a store. Per-site regions made `step` and `swap` below print
//! stale values.
mod support;

const PROGRAM: &str = r#"class Particle:
    x: f64
    vx: f64
    hits: i64

class Pair:
    left: i64
    right: i64

def step(particles: &mut list[Particle]) -> ():
    for p in particles:
        p.x += p.vx
        if p.x < 0.0 or p.x > 10.0:
            p.vx = 0.0 - p.vx
            p.hits += 1

def bump(slot: &mut i64, by: i64) -> i64:
    before = *slot
    *slot = before + by
    *slot * 10 + before

def swap(pair: &mut Pair) -> i64:
    left = pair.left
    pair.left = pair.right
    pair.right = left
    pair.left * 100 + pair.right

def nested(value: &mut Option[Option[i64]], by: i64) -> i64:
    match value:
        case Some(inner):
            match inner:
                case Some(number):
                    *number = *number + by
                    *number
                case Nothing:
                    *inner = Some(by)
                    0
        case Nothing:
            -1

def rows(values: &mut list[i64]) -> i64:
    values[0] = values[1] + 1
    values[1] = values[0] + 1
    first = &mut values[2]
    *first = values[0] + values[1]
    values[2]

def main() -> Result[(), AllocError | IoError]:
    mut particles: list[Particle] = []?
    for i in range(4):
        particles.append(Particle(f64(i * 3), f64(i) - 1.5, 0))?
    for _ in range(9):
        step(&mut particles)
    for p in &particles:
        print(p.x, p.vx, p.hits)?
    mut count = 5
    print(bump(&mut count, 2), bump(&mut count, 3), count)?
    mut pair = Pair(1, 2)
    print(swap(&mut pair), swap(&mut pair), pair.left, pair.right)?
    mut value: Option[Option[i64]] = Some(Some(4))
    print(nested(&mut value, 3), nested(&mut value, 5))?
    mut empty: Option[Option[i64]] = Some(Nothing)
    print(nested(&mut empty, 7), nested(&mut empty, 1))?
    mut values = [1, 2, 3]?
    print(rows(&mut values), values[0], values[1], values[2])?
    Ok(())
"#;

const EXPECTED: &str = "10.5 -1.5 2
0.5 0.5 1
10.5 -0.5 1
-1.5 1.5 2
75 107 10
201 102 1 2
7 12
0 8
7 3 4 7
";

fn plenty(dir: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new(support::plenty_bin())
        .current_dir(dir)
        .env("PLENTY_CACHE", "off")
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn reference_writes_are_visible_to_later_reads() {
    // Field read, store and re-read in one block; writes through locals,
    // nested payload projections and list rows, each read back.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("order.plenty"), PROGRAM).unwrap();
    assert_eq!(plenty(dir.path(), &["order.plenty"]), EXPECTED);
    assert_eq!(plenty(dir.path(), &["--release", "order.plenty"]), EXPECTED);
}

#[test]
fn unprojected_reference_writes_store_directly() {
    // A reference made from an address projects no payload, so its write
    // keeps no old tag bits and needs no read of the slot first.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("order.plenty"), PROGRAM).unwrap();
    plenty(
        dir.path(),
        &[
            "--release",
            "--dump",
            "clif-opt",
            "--dump-filter",
            "step",
            "order.plenty",
            "-o",
            "out",
        ],
    );
    let text = std::fs::read_to_string(dir.path().join("out").join("clif-opt.txt")).unwrap();
    assert!(text.contains("==== step "), "{text}");
    assert!(!text.contains("load.i128"), "{text}");
}
