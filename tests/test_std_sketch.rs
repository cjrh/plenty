//! Sketch invariants, wire validation, bounded growth and allocation failures.
mod support;

fn program(body: &str) -> String {
    let indented = body
        .lines()
        .map(|line| format!("    {line}\n"))
        .collect::<String>();
    format!(
        "import std.sketch as sketch\nimport std.sketch.xor as xor\nimport std.sketch.cuckoo as cuckoo\nimport std.bytes\nfrom std.bytes import BytesError\nfrom std.sketch import SketchError\ndef main() -> Result[(), AllocError | IoError | SketchError | BytesError]:\n{indented}    Ok(())\n"
    )
}

fn native(body: &str, expected: &str) {
    assert_eq!(
        support::run_file_stdout(&program(body)).trim(),
        expected.trim()
    );
}

#[test]
fn hll_merge_duplicates_seed_and_accuracy() {
    native(
        r#"
mut all = sketch.hyperloglog(12, 7u64)?
mut left = sketch.hyperloglog(12, 7u64)?
mut right = sketch.hyperloglog(12, 7u64)?
print(all.estimate() == 0.0)?
for key in range(50000):
    all.add(u64(key))
    all.add(u64(key))
    if key % 2 == 0:
        left.add(u64(key))
    else:
        right.add(u64(key))
left.merge(&right)?
print(left.estimate() == all.estimate(), all.estimate() > 47000.0 and all.estimate() < 53000.0)?
wire = all.to_bytes()?
same = sketch.hyperloglog_from_bytes(&wire)?
print(same.estimate() == all.estimate(), all.relative_standard_error() == 0.01625)?
different = sketch.hyperloglog(12, 8u64)?
match left.merge(&different):
    case Err(_):
        print(True)?
    case Ok(_):
        print(False)?
match sketch.hyperloglog(3):
    case Err(_):
        print(True)?
    case Ok(_):
        print(False)?
match sketch.hyperloglog_from_bytes(&wire, 10):
    case Err(_):
        print(True)?
    case Ok(_):
        print(False)?
"#,
        "True\nTrue True\nTrue True\nTrue\nTrue\nTrue",
    );
}

#[test]
fn stable_hash_and_little_endian_hll_checkpoint_fixture() {
    native(
        r#"
print(sketch.hash_u64(0u64) == 16294208416658607535u64, sketch.hash_u64(42u64) == 13679457532755275413u64)?
gold = b"PSH1\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x03\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02"
mut h = sketch.hyperloglog(4)?
h.add(0u64)
h.add(1u64)
wire = h.to_bytes()?
print(wire.len() == 36 and wire.matches(0, &gold, 0, len(gold))?)?
read = sketch.hyperloglog_from_bytes(&gold)?
print(read.estimate() == h.estimate())?
"#,
        "True True\nTrue\nTrue",
    );
}

#[test]
fn reservoir_quantiles_checkpoint_continuation_and_extremes() {
    native(
        r#"
mut q = sketch.quantiles(512, 77u64)?
for value in range(20000):
    q.add(f64(value))?
print(q.count(), q.retained(), q.quantile(0.0)?, q.quantile(1.0)?)?
print(q.quantile(0.5)? > 8500.0 and q.quantile(0.5)? < 11500.0)?
wire = q.to_bytes()?
mut restored = sketch.quantiles_from_bytes(&wire)?
for value in range(20000, 30000):
    q.add(f64(value))?
    restored.add(f64(value))?
print(q.to_bytes()?.to_list()? == restored.to_bytes()?.to_list()?)?
mut exact = sketch.quantiles(10)?
for value in [9.0, 1.0, 5.0, 3.0]?:
    exact.add(value)?
print(exact.quantile(0.5)?, exact.quantile(0.0)?, exact.quantile(1.0)?)?
mut extremes = sketch.quantiles(8)?
for value in [-1.7976931348623157e308, -0.0, 5e-324, 1.7976931348623157e308]?:
    extremes.add(value)?
raw = extremes.to_bytes()?
roundtrip = sketch.quantiles_from_bytes(&raw)?
print(roundtrip.to_bytes()?.to_list()? == raw.to_list()?)?
"#,
        "20000 512 0.0 19999.0\nTrue\nTrue\n3.0 1.0 9.0\nTrue",
    );
}

#[test]
fn frequent_items_bounds_against_exact_counts() {
    native(
        r#"
mut f = sketch.frequent_items(8)?
mut counts = [0u64 for _ in range(101)]?
for index in range(10000):
    key = 0 if index % 3 == 0 else index % 101
    counts[key] += 1u64
    f.add(u64(key))?
mut correct = True
for key in range(101):
    estimate = f.frequency(u64(key))
    correct = correct and estimate.lower <= counts[key] and counts[key] <= estimate.upper
print(correct, f.count(), f.error_bound() <= 10000u64 // 9u64)?
mut found = False
for item in f.candidates()?:
    if item.key == 0u64:
        found = True
print(found)?
wire = f.to_bytes()?
same = sketch.frequent_items_from_bytes(&wire)?
print(same.to_bytes()?.to_list()? == wire.to_list()?)?
"#,
        "True 10000 True\nTrue\nTrue",
    );
}

#[test]
fn xor_no_false_negatives_duplicates_roundtrip_and_false_positive_rate() {
    native(
        r#"
keys = [u64(index // 2) for index in range(10000)]?
filter = xor.build(&keys, 10000, 99u64)?
mut missing = 0
for key in range(5000):
    if not filter.contains(u64(key)):
        missing += 1
mut positives = 0
for key in range(5000, 55000):
    if filter.contains(u64(key)):
        positives += 1
print(filter.len(), missing, positives > 80 and positives < 400)?
wire = filter.to_bytes()?
same = xor.from_bytes(&wire, 5000)?
print(same.to_bytes()?.to_list()? == wire.to_list()?)?
empty: list[u64] = []?
zero = xor.build(&empty, 0)?
print(zero.len(), zero.contains(0u64))?
"#,
        "5000 0 True\nTrue\n0 False",
    );
}

#[test]
fn xor_construction_across_small_and_composite_block_sizes() {
    native(
        r#"
mut valid = True
for size in [0, 1, 2, 4, 10, 20, 30, 32, 50, 100, 254, 256, 510, 1000]?:
    keys = [u64(key) for key in range(size)]?
    for seed in range(8):
        filter = xor.build(&keys, size, u64(seed))?
        for key in &keys:
            valid = valid and filter.contains(*key)
print(valid)?
"#,
        "True",
    );
}

#[test]
fn cuckoo_growth_deletion_duplicates_and_limit_rollback() {
    native(
        r#"
mut filter = cuckoo.cuckoo(2, 1022, 3u64)?
mut receipts: list[i64] = []?
for key in range(1000):
    receipts.append(filter.insert(u64(key))?)?
mut valid = True
for key in range(1000):
    valid = valid and filter.contains(u64(key))
print(valid, filter.len(), filter.layers() > 1)?
wire = filter.to_bytes()?
mut same = cuckoo.from_bytes(&wire, 1022)?
for key in range(0, 1000, 2):
    valid = valid and same.remove(u64(key), receipts[key])?
for key in range(1, 1000, 2):
    valid = valid and same.contains(u64(key))
print(valid, same.len())?
mut tiny = cuckoo.cuckoo(1, 1)?
for _ in range(4):
    tiny.insert(42u64)?
before = tiny.to_bytes()?
match tiny.insert(99u64):
    case Ok(_):
        print(False)?
    case Err(_):
        print(before.to_list()? == tiny.to_bytes()?.to_list()?)?
for _ in range(3):
    drop(tiny.remove(42u64, 0)?)
print(tiny.contains(42u64), tiny.len(), tiny.remove(42u64, 0)?, tiny.contains(42u64))?
"#,
        "True 1000 True\nTrue 500\nTrue\nTrue 1 True False",
    );
}

#[test]
fn malformed_and_limited_wire_inputs() {
    native(
        r#"
mut rejected = 0
for raw in [b"", b"PSH1", b"PSQ1", b"PSX1", b"PSC1", b"PSF1"]?:
    data = std.bytes.from_bytes(raw)?
    match sketch.hyperloglog_from_bytes(&data):
        case Err(_):
            rejected += 1
        case Ok(_):
            pass
    match sketch.quantiles_from_bytes(&data):
        case Err(_):
            rejected += 1
        case Ok(_):
            pass
    match sketch.frequent_items_from_bytes(&data):
        case Err(_):
            rejected += 1
        case Ok(_):
            pass
    match xor.from_bytes(&data, 100):
        case Err(_):
            rejected += 1
        case Ok(_):
            pass
    match cuckoo.from_bytes(&data, 100):
        case Err(_):
            rejected += 1
        case Ok(_):
            pass
print(rejected)?
mut h = sketch.hyperloglog(4)?.to_bytes()?
h.set(20, 255u8)?
match sketch.hyperloglog_from_bytes(&h):
    case Err(_):
        print(True)?
    case Ok(_):
        print(False)?
"#,
        "30\nTrue",
    );
}

#[test]
fn every_truncated_checkpoint_and_trailing_bytes_are_rejected() {
    for (setup, restore) in [
        (
            "state = sketch.hyperloglog(4)?",
            "sketch.hyperloglog_from_bytes(&part)",
        ),
        (
            "mut state = sketch.quantiles(2)?\nstate.add(1.25)?",
            "sketch.quantiles_from_bytes(&part)",
        ),
        (
            "mut state = sketch.frequent_items(2)?\nstate.add(7u64)?",
            "sketch.frequent_items_from_bytes(&part)",
        ),
        (
            "keys = [1u64, 2u64]?\nstate = xor.build(&keys, 2)?",
            "xor.from_bytes(&part, 2)",
        ),
        (
            "mut state = cuckoo.cuckoo(1, 3)?\nstate.insert(7u64)?",
            "cuckoo.from_bytes(&part, 3)",
        ),
    ] {
        native(
            &format!(
                r#"
{setup}
wire = state.to_bytes()?
mut good = True
for size in range(wire.len()):
    part = wire.slice(0, size)?
    match {restore}:
        case Ok(_):
            good = False
        case Err(_):
            pass
mut part = wire.copy()?
part.push(0u8)?
match {restore}:
    case Ok(_):
        good = False
    case Err(_):
        pass
print(good)?
"#
            ),
            "True",
        );
    }
}

#[test]
fn rejected_updates_preserve_state_and_count_overflow_is_recoverable() {
    native(
        r#"
mut q = sketch.quantiles(1)?
q.add(3.0)?
before = q.to_bytes()?
drop(q.add(f64.NAN))
drop(q.add(f64.INF))
print(before.to_list()? == q.to_bytes()?.to_list()?)?
mut huge = before.copy()?
for at in range(12, 20):
    huge.set(at, 255u8)?
mut restored = sketch.quantiles_from_bytes(&huge)?
match restored.add(4.0):
    case Ok(_):
        print(False)?
    case Err(_):
        print(huge.to_list()? == restored.to_bytes()?.to_list()?)?
mut f = sketch.frequent_items(1)?
f.add(8u64)?
mut raw = f.to_bytes()?
for at in range(12, 20):
    raw.set(at, 255u8)?
for at in range(36, 44):
    raw.set(at, 255u8)?
mut full = sketch.frequent_items_from_bytes(&raw)?
match full.add(8u64):
    case Ok(_):
        print(False)?
    case Err(_):
        print(raw.to_list()? == full.to_bytes()?.to_list()?)?
"#,
        "True\nTrue\nTrue",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn allocation_failures_are_reported_and_updates_need_no_allocation() {
    native(
        r#"
mut h = sketch.hyperloglog(8)?
mut q = sketch.quantiles(32)?
mut f = sketch.frequent_items(8)?
mut c = cuckoo.cuckoo(64, 64)?
other = sketch.hyperloglog(8)?
keys = [u64(index) for index in range(100)]?
filter = xor.build(&keys, 100)?
print("__test_begin_no_allocations__")?
for index in range(100):
    h.add(u64(index))
    q.add(f64(index))?
    f.add(u64(index))?
    c.insert(u64(index))?
h.merge(&other)?
drop(h.estimate())
drop(q.quantile(0.5)?)
drop(f.frequency(2u64))
for index in range(100):
    drop(filter.contains(u64(index)))
    drop(c.contains(u64(index)))
    drop(c.remove(u64(index), 0)?)
print("__test_end_no_allocations__")?
"#,
        "",
    );
    for expression in [
        "sketch.hyperloglog(8)",
        "sketch.quantiles(32)",
        "sketch.frequent_items(8)",
        "xor.build(&keys, 100)",
        "cuckoo.cuckoo(4, 64)",
    ] {
        let mut body = String::from("keys = [u64(index) for index in range(100)]?\n");
        for budget in 0..32 {
            body.push_str(&format!("print(\"__test_fail_allocations_after_{budget}__\")?\nresult{budget} = {expression}\nprint(\"__test_restore_allocations__\")?\nmatch result{budget}:\n    case Ok(_):\n        print(\"ok\")?\n    case Err(error):\n        print(error)?\n"));
        }
        let output = support::run_file_stdout(&program(&body));
        assert!(output.contains("OutOfMemory"));
        assert!(
            output.lines().any(|line| line == "ok"),
            "{expression}: {output}"
        );
        assert_eq!(output.lines().count(), 32);
        assert!(
            output
                .lines()
                .all(|line| line == "ok" || line.contains("OutOfMemory"))
        );
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn checkpoint_allocation_failures_and_failed_growth_preserve_live_state() {
    for (setup, restore) in [
        (
            "state = sketch.hyperloglog(4)?",
            "sketch.hyperloglog_from_bytes(&wire)",
        ),
        (
            "mut state = sketch.quantiles(2)?\nstate.add(1.25)?\nstate.add(2.5)?",
            "sketch.quantiles_from_bytes(&wire)",
        ),
        (
            "mut state = sketch.frequent_items(2)?\nstate.add(7u64)?",
            "sketch.frequent_items_from_bytes(&wire)",
        ),
        (
            "keys = [1u64, 2u64]?\nstate = xor.build(&keys, 2)?",
            "xor.from_bytes(&wire, 2)",
        ),
        (
            "mut state = cuckoo.cuckoo(1, 3)?\nstate.insert(7u64)?",
            "cuckoo.from_bytes(&wire, 3)",
        ),
    ] {
        let mut body = format!("{setup}\nwire = state.to_bytes()?\n");
        for budget in 0..32 {
            for (index, expression) in ["state.to_bytes()", restore].into_iter().enumerate() {
                body.push_str(&format!("print(\"__test_fail_allocations_after_{budget}__\")?\nresult{budget}_{index} = {expression}\nprint(\"__test_restore_allocations__\")?\nmatch result{budget}_{index}:\n    case Ok(_):\n        print(\"ok\")?\n    case Err(error):\n        print(error)?\n"));
            }
        }
        body.push_str("print(state.to_bytes()?.to_list()? == wire.to_list()?)?\n");
        let output = support::run_file_stdout(&program(&body));
        assert_eq!(output.lines().count(), 65);
        assert!(output.contains("OutOfMemory"));
        assert!(output.contains("ok\n"));
        assert!(output.trim_end().ends_with("True"));
        assert!(
            output
                .lines()
                .all(|line| line == "ok" || line == "True" || line.contains("OutOfMemory"))
        );
    }
    for budget in 0..8 {
        native(
            &format!(
                r#"
mut filter = cuckoo.cuckoo(1, 3)?
for _ in range(4):
    filter.insert(42u64)?
before = filter.to_bytes()?
print("__test_fail_allocations_after_{budget}__")?
result = filter.insert(99u64)
print("__test_restore_allocations__")?
match result:
    case Ok(layer):
        print(layer == 1 and filter.len() == 5u64 and filter.contains(42u64) and filter.contains(99u64))?
    case Err(_):
        print(before.to_list()? == filter.to_bytes()?.to_list()?)?
"#
            ),
            "True",
        );
    }
}
