//! Public portable vectors are differential-tested against scalar lane rules.
mod support;
use rstest::rstest;
use std::process::{Command, Output};

fn run(source: &str, release: bool) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("simd.plenty");
    std::fs::write(&path, source).unwrap();
    let mut command = Command::new(support::plenty_bin());
    command.env("PLENTY_CACHE", "off");
    if release {
        command.arg("--release");
    }
    command.arg(path).output().unwrap()
}

fn passes(source: &str, release: bool) {
    let out = run(source, release);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"ok\n");
}

#[rstest]
#[case(false)]
#[case(true)]
fn integer_lanes_match_scalar_arithmetic_and_comparisons(#[case] release: bool) {
    let mut source =
        String::from("import std.simd as simd\nfrom std.test import check\ndef main() -> ():\n");
    for (ty, values) in [
        (
            "i32",
            "[i32.MIN, -1i32, 0i32, 1i32, 2147483646i32, i32.MAX]",
        ),
        (
            "u32",
            "[0u32, 1u32, 2147483647u32, 2147483648u32, 4294967294u32, u32.MAX]",
        ),
    ] {
        source.push_str(&format!("    values_{ty} = {values}.unwrap()\n    for left in &values_{ty}:\n        for right in &values_{ty}:\n            a = simd.{ty}x4.Lanes(*left, *right, *left, *right)\n            b = simd.{ty}x4.Lanes(*right, *left, *right, *left)\n"));
        for method in ["wrapping_add", "wrapping_sub", "wrapping_mul"] {
            source.push_str(&format!("            result_{method} = a.{method}(b)\n            check(result_{method}.to_tuple() == ((*left).{method}(*right), (*right).{method}(*left), (*left).{method}(*right), (*right).{method}(*left)), \"{ty} {method}\").unwrap()\n"));
        }
        source.push_str("            check(a.wrapping_sum() == (*left).wrapping_add(*right).wrapping_add(*left).wrapping_add(*right), \"sum\").unwrap()\n");
        for (method, operator) in [
            ("eq", "=="),
            ("ne", "!="),
            ("lt", "<"),
            ("le", "<="),
            ("gt", ">"),
            ("ge", ">="),
        ] {
            source.push_str(&format!("            mask_{method} = a.simd_{method}(b)\n            expected_{method} = simd.mask32x4.Lanes(*left {operator} *right, *right {operator} *left, *left {operator} *right, *right {operator} *left)\n            check(mask_{method} == expected_{method}, \"{ty} {method}\").unwrap()\n            chosen_{method} = mask_{method}.select(a, b)\n            check(chosen_{method}.lane(0).unwrap() == (*left if *left {operator} *right else *right), \"selected lane\").unwrap()\n"));
        }
    }
    source.push_str("    print(\"ok\").unwrap()\n");
    passes(&source, release);
}

#[rstest]
#[case(false)]
#[case(true)]
fn float_lanes_preserve_ieee_comparisons_and_ordered_folds(#[case] release: bool) {
    let mut source =
        String::from("import std.simd as simd\nfrom std.test import check\ndef main() -> ():\n");
    for (ty, vector, count, tiny) in [
        ("f32", "f32x4", 4, "1e-40f32"),
        ("f64", "f64x2", 2, "1e-310"),
    ] {
        source.push_str(&format!("    values_{ty} = [{ty}.NAN, {ty}.INF, -{ty}.INF, 0.0{ty}, -0.0{ty}, 1.0{ty}, -2.0{ty}, {tiny}].unwrap()\n    for left in &values_{ty}:\n        for right in &values_{ty}:\n            a = simd.splat_{vector}(*left)\n            b = simd.splat_{vector}(*right)\n"));
        for (method, operator) in [("add", "+"), ("sub", "-"), ("mul", "*"), ("div", "/")] {
            source.push_str(&format!("            expected_{method} = *left {operator} *right\n            result_{method} = a.{method}(b).lane({}).unwrap()\n            check(result_{method} == expected_{method} or (result_{method} != result_{method} and expected_{method} != expected_{method}), \"{vector} {method}\").unwrap()\n            if result_{method} == 0.0{ty}:\n                check((1.0{ty}).copysign(result_{method}) == (1.0{ty}).copysign(expected_{method}), \"signed zero\").unwrap()\n", count - 1));
        }
        for (method, operator) in [
            ("eq", "=="),
            ("ne", "!="),
            ("lt", "<"),
            ("le", "<="),
            ("gt", ">"),
            ("ge", ">="),
        ] {
            source.push_str(&format!("            mask_{method} = a.simd_{method}(b)\n            check(mask_{method}.count() == ({count} if *left {operator} *right else 0), \"{vector} {method}\").unwrap()\n            result_{method} = mask_{method}.select(a, b).lane(0).unwrap()\n            expected_{method} = *left if *left {operator} *right else *right\n            check(result_{method} == expected_{method} or (result_{method} != result_{method} and expected_{method} != expected_{method}), \"float select\").unwrap()\n"));
        }
    }
    source.push_str("    ordered = simd.f32x4.Lanes(1e20f32, 1.0f32, -1e20f32, 1.0f32)\n    check(ordered.sum_ordered(0.0f32) == 1.0f32, \"no reassociation\").unwrap()\n    print(\"ok\").unwrap()\n");
    passes(&source, release);
}

#[rstest]
#[case(false)]
#[case(true)]
fn memory_bounds_copying_masks_and_nested_values(#[case] release: bool) {
    passes(
        r#"import std.simd as simd
from std.test import check
class Holder:
    value: simd.i32x4
def echo(value: simd.i32x4) -> simd.i32x4:
    value
def main() -> ():
    v = simd.i32x4.Lanes(1, 2, 3, 4)
    saved = v
    holder = Holder(echo(v))
    rows = [saved, holder.value].unwrap()
    check(rows[1] == v, "copyable values in storage").unwrap()
    wrapped = Some(v)
    match wrapped:
        case Some(value):
            check(value.wrapping_add(v).to_tuple() == (2i32, 4i32, 6i32, 8i32), "projected receiver").unwrap()
        case Nothing:
            check(False, "missing vector").unwrap()
    mut values = [10i32, 20i32, 30i32, 40i32, 50i32].unwrap()
    check(simd.load_i32x4(&values, 1).unwrap().to_tuple() == (20i32, 30i32, 40i32, 50i32), "offset load").unwrap()
    check(v.store(&mut values, 1), "store fits").unwrap()
    check(values == [10i32, 1i32, 2i32, 3i32, 4i32].unwrap(), "stored lanes").unwrap()
    no_vector: Option[simd.i32x4] = Nothing
    no_lane: Option[i32] = Nothing
    for offset in [-1, 2, i64.MAX].unwrap():
        check(simd.load_i32x4(&values, offset) == no_vector, "invalid load").unwrap()
        check(not v.store(&mut values, offset), "invalid store").unwrap()
    check(values == [10i32, 1i32, 2i32, 3i32, 4i32].unwrap(), "no partial write").unwrap()
    check(v.lane(-1) == no_lane and v.lane(4) == no_lane, "lane bounds").unwrap()
    yes = simd.mask32x4.Lanes(True, True, True, True)
    no = yes.invert()
    mixed = simd.mask32x4.Lanes(True, False, True, False)
    check(yes.all() and not no.any() and mixed.count() == 2, "mask summaries").unwrap()
    check(mixed.and_mask(yes) == mixed and mixed.or_mask(no) == mixed, "mask combine").unwrap()
    check(mixed.xor_mask(yes) == mixed.invert(), "mask xor").unwrap()
    print("ok").unwrap()
"#,
        release,
    );
}

#[rstest]
#[case(false)]
#[case(true)]
fn checked_integer_methods_still_trap_at_the_call(#[case] release: bool) {
    let output = run(
        "import std.simd as simd\ndef main() -> ():\n    a = simd.i32x4.Lanes(0, i32.MAX, 0, 0)\n    b = simd.splat_i32x4(1)\n    drop(a.add(b))\n",
        release,
    );
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("overflow"), "{error}");
    assert!(error.contains("simd.plenty:5"), "{error}");
}

#[test]
fn incompatible_masks_and_vector_shapes_are_rejected() {
    for source in [
        "import std.simd as s\ndef main() -> ():\n    a = s.splat_i32x4(1)\n    b = s.splat_u32x4(1u32)\n    drop(a.wrapping_add(b))\n",
        "import std.simd as s\ndef main() -> ():\n    m = s.mask64x2.Lanes(True, False)\n    a = s.splat_i32x4(1)\n    drop(m.select(a, a))\n",
    ] {
        let output = run(source, true);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("expected"));
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn distinct_float_lanes_and_every_mask_lane(#[case] release: bool) {
    passes(
        r#"import std.simd as s
from std.test import check
def main() -> ():
    a = s.f32x4.Lanes(1.0f32, 20.0f32, 3.0f32, 40.0f32)
    b = s.f32x4.Lanes(10.0f32, 2.0f32, 30.0f32, 4.0f32)
    m = a.simd_lt(b)
    check(m == s.mask32x4.Lanes(True, False, True, False), "mixed float comparison").unwrap()
    check(m.select(a, b).to_tuple() == (1.0f32, 2.0f32, 3.0f32, 4.0f32), "all selected lanes").unwrap()
    check(a.add(b).to_tuple() == (11.0f32, 22.0f32, 33.0f32, 44.0f32), "distinct addition").unwrap()
    x = s.f64x2.Lanes(10.0, -20.0)
    y = s.f64x2.Lanes(2.0, 4.0)
    n = x.simd_gt(y)
    check(n == s.mask64x2.Lanes(True, False), "two lane comparison").unwrap()
    check(n.select(x, y).to_tuple() == (10.0, 4.0), "two lane selection").unwrap()
    check(x.div(y).to_tuple() == (5.0, -5.0), "distinct division").unwrap()
    check(n.any() and not n.all() and n.count() == 1, "two lane summaries").unwrap()
    check(n.or_mask(n.invert()).all(), "or complement").unwrap()
    check(not n.and_mask(n.invert()).any(), "and complement").unwrap()
    check(n.xor_mask(n).count() == 0, "xor self").unwrap()
    print("ok").unwrap()
"#,
        release,
    );
}

#[rstest]
#[case(false)]
#[case(true)]
fn every_shape_checks_empty_short_and_complete_buffers(#[case] release: bool) {
    for (vector, ty, count, literal) in [
        ("i32x4", "i32", 4, "7i32"),
        ("u32x4", "u32", 4, "7u32"),
        ("f32x4", "f32", 4, "7.0f32"),
        ("f64x2", "f64", 2, "7.0"),
    ] {
        passes(
            &format!(
                "import std.simd as s\nfrom std.test import check\ndef main() -> ():\n    v = s.splat_{vector}({literal})\n    missing: Option[s.{vector}] = Nothing\n    for length in range({count} + 1):\n        mut values: list[{ty}] = [{literal} for i in range(length)].unwrap()\n        if length < {count}:\n            check(s.load_{vector}(&values) == missing, \"short load\").unwrap()\n            check(not v.store(&mut values, 0), \"short store\").unwrap()\n        else:\n            check(s.load_{vector}(&values).unwrap() == v, \"full load\").unwrap()\n            check(v.store(&mut values, 0), \"full store\").unwrap()\n        check(len(values) == length, \"length unchanged\").unwrap()\n    print(\"ok\").unwrap()\n"
            ),
            release,
        );
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn checked_subtraction_multiplication_and_unsigned_overflow(#[case] release: bool) {
    for (vector, left, method, right) in [
        ("i32x4", "i32.MIN", "sub", "1i32"),
        ("i32x4", "i32.MAX", "mul", "2i32"),
        ("u32x4", "0u32", "sub", "1u32"),
        ("u32x4", "u32.MAX", "add", "1u32"),
        ("u32x4", "u32.MAX", "mul", "2u32"),
    ] {
        let out = run(
            &format!(
                "import std.simd as s\ndef main() -> ():\n    a = s.splat_{vector}({left})\n    b = s.splat_{vector}({right})\n    drop(a.{method}(b))\n"
            ),
            release,
        );
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("overflow"));
    }
}

#[test]
fn vector_selection_helper_is_private() {
    let out = run(
        "import std.simd as s\ndef main() -> ():\n    a = s.splat_i32x4(1)\n    m = s.mask32x4.Lanes(True, False, True, False)\n    drop(a.selected(m, a))\n",
        false,
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("private"));
}

#[rstest]
#[case(false)]
#[case(true)]
fn structural_selection_keeps_user_effects(#[case] release: bool) {
    let out = run(
        r#"import std.simd as s
pub class Custom:
    value: i32
    pub def selected(self, mask: &s.mask32x4, other: &Custom) -> Custom:
        print("custom selection").unwrap()
        Custom(self.value if mask.any() else other.value)
def main() -> ():
    a = Custom(1)
    b = Custom(2)
    m = s.mask32x4.Lanes(False, False, False, False)
    print(m.select(a, b).value).unwrap()
"#,
        release,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"custom selection\n2\n");
}

#[test]
#[cfg(target_arch = "x86_64")]
fn optimized_operations_emit_vectors_while_checked_arithmetic_stays_scalar() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("simd.plenty");
    std::fs::write(&path, "import std.simd as s\ndef native(a: &s.i32x4, b: &s.i32x4) -> s.i32x4:\n    a.wrapping_add(b)\ndef checked(a: &s.i32x4, b: &s.i32x4) -> s.i32x4:\n    a.add(b)\ndef main() -> ():\n    a = s.splat_i32x4(1)\n    drop(native(a, a))\n    drop(checked(a, a))\n").unwrap();
    for (function, vector) in [
        ("std.simd.i32x4.wrapping_add", true),
        ("std.simd.i32x4.add", false),
    ] {
        let out = Command::new(support::plenty_bin())
            .args([
                "--release",
                "--dump",
                "clif,asm",
                "--dump-std",
                "--dump-filter",
                function,
            ])
            .arg(&path)
            .args(["-o", "-"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let dump = String::from_utf8_lossy(&out.stdout);
        assert_eq!(dump.contains("splat.i32x4"), vector, "{dump}");
        if vector {
            assert!(dump.contains("paddd"), "{dump}");
        }
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn vector_locals_preserve_control_flow_borrowing_and_copy_boundaries(#[case] release: bool) {
    passes(
        r#"import std.simd as s
from std.test import check
def first(value: &s.i32x4) -> &i32:
    match value:
        case s.i32x4.Lanes(a, b, c, d):
            a
def echo(value: s.i32x4) -> s.i32x4:
    value
def choose(flag: bool, a: s.i32x4, b: s.i32x4) -> s.i32x4:
    a if flag else b
def local_choice(flag: bool) -> s.i32x4:
    a = s.splat_i32x4(10)
    b = s.splat_i32x4(20)
    chosen = a if flag else b
    chosen.wrapping_add(s.splat_i32x4(1))
def main() -> ():
    one = s.splat_i32x4(1)
    for count in range(5):
        mut value = s.splat_i32x4(0)
        for outer in range(count):
            for inner in range(4):
                if inner == 0:
                    continue
                if inner == 3:
                    break
                if inner == 1:
                    value = value.wrapping_add(one)
                else:
                    match outer % 2:
                        case 0:
                            value = value.wrapping_add(one)
                        case _:
                            value = value.wrapping_sub(one)
        expected = i32((count + 1) // 2) * 2i32
        check(value.to_tuple() == (expected, expected, expected, expected), "loop and branch joins").unwrap()
    mut original = s.i32x4.Lanes(1, 2, 3, 4)
    saved = original
    original = original.wrapping_add(one)
    check(saved.to_tuple() == (1i32, 2i32, 3i32, 4i32), "saved copy").unwrap()
    shared = &original
    field = first(shared)
    check(*field == 2i32 and shared.lane(1).unwrap() == 3i32, "shared snapshots").unwrap()
    check(echo(original) == original, "by value ABI").unwrap()
    check(choose(True, original, saved) == original, "value merge true").unwrap()
    check(choose(False, original, saved) == saved, "value merge false").unwrap()
    check(local_choice(True).lane(0).unwrap() == 11i32, "promoted merge and return true").unwrap()
    check(local_choice(False).lane(0).unwrap() == 21i32, "promoted merge and return false").unwrap()
    mut mutable = s.i32x4.Lanes(1, 2, 3, 4)
    match &mut mutable:
        case s.i32x4.Lanes(a, b, c, d):
            *a = 10i32
            *d = 40i32
    check(mutable.wrapping_add(one).to_tuple() == (11i32, 3i32, 4i32, 41i32), "mutable projection fallback").unwrap()
    mut captured = s.splat_i32x4(10)
    mut change = def [&mut captured]() -> ():
        captured = captured.wrapping_add(s.splat_i32x4(2))
    change()
    change()
    drop(change)
    check(captured.to_tuple() == (14i32, 14i32, 14i32, 14i32), "mutable capture fallback").unwrap()
    print("ok").unwrap()
"#,
        release,
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn lane_operations_and_list_transfers_do_not_allocate() {
    support::assert_visible_stdout(
        r#"import std.simd as s
def main() -> ():
    mut values = [1i32, 2i32, 3i32, 4i32].unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    a = s.load_i32x4(&values).unwrap()
    b = a.wrapping_mul(s.splat_i32x4(2))
    m = b.simd_gt(a)
    selected = m.select(b, a)
    stored = selected.store(&mut values, 0)
    print("__test_restore_allocations__").unwrap()
    print(stored, values).unwrap()
"#,
        "True [2, 4, 6, 8]",
    );
}
