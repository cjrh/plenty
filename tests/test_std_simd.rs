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
    if !release {
        command.arg("--fast-compile");
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

fn separate_integer_reductions() -> String {
    let mut source = String::from("import std.simd as s\nfrom std.test import check\n");
    for ty in ["i32", "u32"] {
        source.push_str(&format!(
            "def sum_pair_{ty}(a: &s.{ty}x4, b: &s.{ty}x4) -> {ty}:\n    a.wrapping_sum().wrapping_add(b.wrapping_sum())\ndef loop_{ty}(count: i64, seed: {ty}) -> {ty}:\n    mut a = s.splat_{ty}x4(seed)\n    mut b = s.splat_{ty}x4(seed.wrapping_add(1{ty}))\n    step = s.splat_{ty}x4(3{ty})\n    for i in range(count):\n        a = a.wrapping_mul(step).wrapping_add(step)\n        b = b.wrapping_mul(step).wrapping_add(step)\n    a.wrapping_sum().wrapping_add(b.wrapping_sum())\n"
        ));
    }
    source.push_str("def main() -> ():\n");
    for ty in ["i32", "u32"] {
        source.push_str(&format!(
            "    seeds_{ty} = [{ty}.MIN, {ty}.MAX, 0{ty}, 1{ty}].unwrap()\n    for seed in &seeds_{ty}:\n        a = s.splat_{ty}x4(*seed)\n        b = s.splat_{ty}x4(1{ty})\n        check(sum_pair_{ty}(a, b) == (*seed).wrapping_mul(4{ty}).wrapping_add(4{ty}), \"borrowed reductions\").unwrap()\n        for count in range(18):\n            mut left = *seed\n            mut right = (*seed).wrapping_add(1{ty})\n            for i in range(count):\n                left = left.wrapping_mul(3{ty}).wrapping_add(3{ty})\n                right = right.wrapping_mul(3{ty}).wrapping_add(3{ty})\n            expected = left.wrapping_mul(4{ty}).wrapping_add(right.wrapping_mul(4{ty}))\n            check(loop_{ty}(count, *seed) == expected, \"separate loop reductions\").unwrap()\n"
        ));
    }
    source.push_str("    print(\"ok\").unwrap()\n");
    source
}

#[rstest]
#[case(false)]
#[case(true)]
fn separate_integer_reductions_preserve_wrapping_and_borrowed_values(#[case] release: bool) {
    passes(&separate_integer_reductions(), release);
}

#[test]
#[cfg(target_arch = "x86_64")]
fn optimized_integer_reductions_have_no_scalar_abi_calls() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reductions.plenty");
    std::fs::write(&path, separate_integer_reductions()).unwrap();
    for ty in ["i32", "u32"] {
        let out = Command::new(support::plenty_bin())
            .env("PLENTY_CACHE", "off")
            .args(["--release", "--dump", "clif-opt", "--dump-filter"])
            .arg(format!("sum_pair_{ty}"))
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
        assert!(dump.contains("iadd"), "{dump}");
        assert!(!dump.contains("call "), "{dump}");
    }
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
fn integer_methods_return_typed_overflow_in_lane_order(#[case] release: bool) {
    let mut source =
        String::from("import std.simd as simd\nfrom std.test import check\ndef main() -> ():\n");
    let mut append_case = |body: String| {
        source.push_str("    if True:\n");
        for line in body.lines() {
            source.push_str("    ");
            source.push_str(line);
            source.push('\n');
        }
    };
    for (ty, max, min) in [("i32", "i32.MAX", "i32.MIN"), ("u32", "u32.MAX", "u32.MIN")] {
        for (method, left, right, wrapped) in [
            ("add", max, format!("1{ty}"), min.to_owned()),
            ("sub", min, format!("1{ty}"), max.to_owned()),
            (
                "mul",
                max,
                format!("2{ty}"),
                format!("({max}).wrapping_mul(2{ty})"),
            ),
        ] {
            for lane in 0..4 {
                let mut lanes = vec![
                    if method == "sub" {
                        right.clone()
                    } else {
                        format!("0{ty}")
                    };
                    4
                ];
                lanes[lane] = left.to_owned();
                append_case(format!(
                    "    a = simd.{ty}x4.Lanes({})\n    b = simd.splat_{ty}x4({right})\n    match a.{method}(b):\n        case Err(error):\n            check(error.wrapped == {wrapped}, \"typed wrapped payload\").unwrap()\n        case Ok(_):\n            check(False, \"expected overflow\").unwrap()\n",
                    lanes.join(", ")
                ));
            }
        }
        append_case(format!(
            "    mut a = simd.{ty}x4.Lanes({max}, {max}, 0{ty}, 0{ty})\n    mut b = simd.{ty}x4.Lanes(1{ty}, 2{ty}, 0{ty}, 0{ty})\n    match a.add(b):\n        case Err(error):\n            check(error.wrapped == ({max}).wrapping_add(1{ty}), \"first failing lane\").unwrap()\n        case Ok(_):\n            check(False, \"expected overflow\").unwrap()\n    a = simd.splat_{ty}x4(3{ty})\n    b = simd.splat_{ty}x4(2{ty})\n    check(a.add(b).unwrap().to_tuple() == (5{ty}, 5{ty}, 5{ty}, 5{ty}), \"add\").unwrap()\n    check(a.sub(b).unwrap().to_tuple() == (1{ty}, 1{ty}, 1{ty}, 1{ty}), \"sub\").unwrap()\n    check(a.mul(b).unwrap().to_tuple() == (6{ty}, 6{ty}, 6{ty}, 6{ty}), \"mul\").unwrap()\n"
        ));
    }
    source.push_str("    print(\"ok\").unwrap()\n");
    passes(&source, release);
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
        let stop = count + 1;
        passes(
            &format!(
                "import std.simd as s\nfrom std.test import check\ndef main() -> ():\n    v = s.splat_{vector}({literal})\n    missing: Option[s.{vector}] = Nothing\n    for length in range({stop}):\n        mut values: list[{ty}] = [{literal} for i in range(length)].unwrap()\n        if length < {count}:\n            check(s.load_{vector}(&values) == missing, \"short load\").unwrap()\n            check(not v.store(&mut values, 0), \"short store\").unwrap()\n        else:\n            check(s.load_{vector}(&values).unwrap() == v, \"full load\").unwrap()\n            check(v.store(&mut values, 0), \"full store\").unwrap()\n        check(len(values) == length, \"length unchanged\").unwrap()\n    print(\"ok\").unwrap()\n"
            ),
            release,
        );
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn integer_overflow_results_can_be_handled_in_an_unchecked_caller(#[case] release: bool) {
    for (vector, left, method, right) in [
        ("i32x4", "i32.MIN", "sub", "1i32"),
        ("i32x4", "i32.MAX", "mul", "2i32"),
        ("u32x4", "0u32", "sub", "1u32"),
        ("u32x4", "u32.MAX", "add", "1u32"),
        ("u32x4", "u32.MAX", "mul", "2u32"),
    ] {
        passes(
            &format!(
                "import std.simd as s\ndef main() -> ():\n    # The caller mode does not alter the callee Result contract.\n    unchecked:\n        a = s.splat_{vector}({left})\n        b = s.splat_{vector}({right})\n        match a.{method}(b):\n            case Err(_):\n                print(\"ok\").unwrap()\n            case Ok(_):\n                print(\"wrong result\").unwrap()\n"
            ),
            release,
        );
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
    std::fs::write(&path, "import std.simd as s\ndef native(a: &s.i32x4, b: &s.i32x4) -> s.i32x4:\n    a.wrapping_add(b)\ndef checked(a: &s.i32x4, b: &s.i32x4) -> Result[s.i32x4, OverflowError[i32]]:\n    a.add(b)\ndef main() -> ():\n    a = s.splat_i32x4(1)\n    drop(native(a, a))\n    drop(checked(a, a))\n").unwrap();
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
        // Constructors may assemble vectors even in the scalar checked body;
        // only wrapping addition may become packed arithmetic.
        assert_eq!(dump.contains("paddd"), vector, "{dump}");
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
def main() -> Result[(), OverflowError[i64] | OverflowError[i32] | ZeroDivisionError]:
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
    Ok(())
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

#[cfg(feature = "runtime-checks")]
#[test]
fn indexed_view_accesses_do_not_allocate() {
    support::assert_visible_stdout(
        r#"import std.simd as s
def main() -> ():
    mut values = [1.0, 2.0, 3.0, 4.0, 5.0].unwrap()
    print("__test_fail_allocations_after_0__").unwrap()
    for i in range(s.len_f64x2(&values)):
        v = s.get_f64x2(&values, i)
        v.add(v).set(&mut values, i)
    tail = s.tail_f64x2(&values)
    print("__test_restore_allocations__").unwrap()
    print(tail, values).unwrap()
"#,
        "4 [2.0, 4.0, 6.0, 8.0, 5.0]",
    );
}

#[cfg(feature = "runtime-checks")]
#[test]
fn fallible_integer_operations_allocate_nothing() {
    let source = r#"import std.simd as simd
def main() -> Result[(), IoError]:
    a = simd.splat_i32x4(i32.MAX)
    b = simd.splat_i32x4(2i32)
    print("__test_begin_no_allocations__")?
    overflow = a.add(b)
    product = b.mul(b)
    print("__test_end_no_allocations__")?
    print(overflow.is_err(), product.unwrap().to_tuple())?
    Ok(())
"#;
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.plenty");
        let output = dir.path().join("program");
        std::fs::write(&input, source).unwrap();
        plenty::compile_file_to_executable_with_options(
            &input,
            &output,
            None,
            &plenty::CompileOptions {
                optimize,
                test_hooks: true,
                ..Default::default()
            },
        )
        .unwrap();
        let result = support::assert_success(Command::new(output).output().unwrap());
        let visible = String::from_utf8(result.stdout)
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("__test_"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(visible, "True (4, 4, 4, 4)");
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn integer_result_callbacks_keep_their_own_contract(#[case] release: bool) {
    passes(
        r#"import std.simd as simd
from std.test import check

def named(value: simd.i32x4) -> Result[simd.i32x4, OverflowError[i32]]:
    value.add(simd.splat_i32x4(1i32))

def invoke[F: Callable[[simd.i32x4], Result[simd.i32x4, OverflowError[i32]]]](callback: &F, value: simd.i32x4) -> Result[simd.i32x4, OverflowError[i32]]:
    callback(value)

def main() -> ():
    callback = unchecked:
        def(value: simd.i32x4) -> Result[simd.i32x4, OverflowError[i32]]:
            # This primitive arithmetic belongs to the callback, not its
            # unchecked definition or invocation context.
            adjustment = value.lane(0).unwrap() + 1i32
            value.add(simd.splat_i32x4(adjustment))
    unchecked:
        match invoke(&named, simd.splat_i32x4(i32.MAX)):
            case Err(error):
                check(error.wrapped == i32.MIN, "named callback payload").unwrap()
            case Ok(_):
                check(False, "missing named callback error").unwrap()
        match invoke(&callback, simd.splat_i32x4(i32.MAX)):
            case Err(error):
                check(error.wrapped == i32.MIN, "closure callback payload").unwrap()
            case Ok(_):
                check(False, "missing closure callback error").unwrap()
        check(invoke(&named, simd.splat_i32x4(1i32)).unwrap().to_tuple() == (2i32, 2i32, 2i32, 2i32), "named success").unwrap()
        check(invoke(&callback, simd.splat_i32x4(1i32)).unwrap().to_tuple() == (3i32, 3i32, 3i32, 3i32), "closure success").unwrap()
    print("ok").unwrap()
"#,
        release,
    );
}

#[rstest]
#[case(false)]
#[case(true)]
fn square_roots_match_scalar_lanes_and_preserve_zero_sign(#[case] release: bool) {
    for (ty, vector, count, tiny) in [
        ("f32", "f32x4", 4, "1e-40f32"),
        ("f64", "f64x2", 2, "1e-310"),
    ] {
        let lanes = (0..count)
            .map(|i| format!("values[(start + {i}) % len(values)]"))
            .collect::<Vec<_>>()
            .join(", ");
        passes(
            &format!(
                r#"import std.simd as s
from std.test import check
def borrowed(value: &s.{vector}) -> s.{vector}:
    value.sqrt()
def main() -> ():
    values = [{ty}.NAN, {ty}.INF, -{ty}.INF, 0.0{ty}, -0.0{ty}, 2.0{ty}, -2.0{ty}, {tiny}, 1e30{ty}].unwrap()
    unchecked trap:
        for start in range(len(values)):
            v = s.{vector}.Lanes({lanes})
            saved = v
            direct = v.sqrt()
            indirect = borrowed(&v)
            for lane in range({count}):
                expected = values[(start + lane) % len(values)].sqrt()
                a = direct.lane(lane).unwrap()
                b = indirect.lane(lane).unwrap()
                check(a == expected or (a != a and expected != expected), "direct sqrt").unwrap()
                check(b == expected or (b != b and expected != expected), "borrowed sqrt").unwrap()
                if expected == 0.0{ty}:
                    check((1.0{ty}).copysign(a) == (1.0{ty}).copysign(expected), "direct zero sign").unwrap()
                    check((1.0{ty}).copysign(b) == (1.0{ty}).copysign(expected), "borrowed zero sign").unwrap()
                original = saved.lane(lane).unwrap()
                input = values[(start + lane) % len(values)]
                check(original == input or (original != original and input != input), "input unchanged").unwrap()
    print("ok").unwrap()
"#
            ),
            release,
        );
    }
}

// The bound holds for the processor's estimate and for the scalar body
// alike; only special lanes have results exact enough to compare.
#[rstest]
#[case(false)]
#[case(true)]
fn reciprocal_square_root_estimate_is_within_its_bound(#[case] release: bool) {
    passes(
        r#"import std.simd as s
from std.test import check
def borrowed(value: &s.f32x4) -> s.f32x4:
    value.reciprocal_sqrt_estimate()
def main() -> ():
    bound = 1.5 / 4096.0
    mut x = 1e-30f32
    while x < 1e30f32:
        v = s.f32x4.Lanes(x, x * 1.1f32, x * 1.3f32, x * 1.7f32)
        direct = v.reciprocal_sqrt_estimate()
        indirect = borrowed(&v)
        for lane in range(4):
            input = f64(v.lane(lane).unwrap())
            for estimate in [direct.lane(lane).unwrap(), indirect.lane(lane).unwrap()].unwrap():
                product = f64(estimate) * input.sqrt()
                check(product >= 1.0 - bound and product <= 1.0 + bound, "relative error").unwrap()
        x = x * 1.37f32
    special = s.f32x4.Lanes(0.0f32, -0.0f32, f32.INF, -1.0f32).reciprocal_sqrt_estimate()
    check(special.lane(0).unwrap() == f32.INF, "zero").unwrap()
    check(special.lane(1).unwrap() == -f32.INF, "negative zero").unwrap()
    check(special.lane(2).unwrap() == 0.0f32, "infinity").unwrap()
    negative = special.lane(3).unwrap()
    check(negative != negative, "negative").unwrap()
    nan = s.splat_f32x4(f32.NAN).reciprocal_sqrt_estimate().lane(0).unwrap()
    check(nan != nan, "nan").unwrap()
    print("ok").unwrap()
"#,
        release,
    );
}

#[rstest]
#[case(false)]
#[case(true)]
fn lane_conversions_match_scalar_casts(#[case] release: bool) {
    passes(
        r#"import std.simd as s
from std.test import check
def narrow(value: &s.f64x2) -> s.f32x4:
    value.to_f32x4()
def widen(value: &s.f32x4) -> s.f64x2:
    value.to_f64x2()
def same32(a: f32, b: f32) -> bool:
    a == b and (1.0f32).copysign(a) == (1.0f32).copysign(b) or (a != a and b != b)
def same64(a: f64, b: f64) -> bool:
    a == b and (1.0).copysign(a) == (1.0).copysign(b) or (a != a and b != b)
def main() -> ():
    wide = [f64.NAN, f64.INF, -f64.INF, 0.0, -0.0, 0.1, -2.5, 1e-310, 1e-50, 1e300, -1e300, 16777217.0].unwrap()
    for a in &wide:
        for b in &wide:
            v = s.f64x2.Lanes(*a, *b)
            for result in [v.to_f32x4(), narrow(&v)].unwrap():
                (r0, r1, r2, r3) = result.to_tuple()
                check(same32(r0, f32(*a)) and same32(r1, f32(*b)), "narrowed lanes").unwrap()
                check(same32(r2, 0.0f32) and same32(r3, 0.0f32), "upper lanes are zero").unwrap()
    narrow_values = [f32.NAN, f32.INF, -f32.INF, 0.0f32, -0.0f32, 0.1f32, -2.5f32, 1e-40f32, 3e38f32].unwrap()
    for a in &narrow_values:
        for b in &narrow_values:
            v = s.f32x4.Lanes(*a, *b, f32.NAN, 7.0f32)
            for result in [v.to_f64x2(), widen(&v)].unwrap():
                (r0, r1) = result.to_tuple()
                check(same64(r0, f64(*a)) and same64(r1, f64(*b)), "widened lanes").unwrap()
    print("ok").unwrap()
"#,
        release,
    );
}

#[test]
#[cfg(target_arch = "x86_64")]
fn reciprocal_square_root_estimate_lowers_to_the_estimate_instruction() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("estimate.plenty");
    std::fs::write(&input, "import std.simd as s\ndef kernel(a: &s.f64x2) -> s.f64x2:\n    d2 = a.mul(a)\n    y = d2.to_f32x4().reciprocal_sqrt_estimate().to_f64x2()\n    y.mul(d2)\ndef main() -> ():\n    a = s.splat_f64x2(4.0)\n    print(kernel(&a).to_tuple()).unwrap()\n").unwrap();
    let out = Command::new(support::plenty_bin())
        .env("PLENTY_CACHE", "off")
        .args(["--dump", "clif-opt,asm", "--dump-filter", "kernel"])
        .arg(input)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dump = String::from_utf8_lossy(&out.stdout);
    for opcode in [
        "fvdemote",
        "x86_rsqrtps",
        "fvpromote_low",
        "cvtpd2ps",
        "rsqrtps",
        "cvtps2pd",
    ] {
        assert!(dump.contains(opcode), "{opcode}: {dump}");
    }
    assert!(!dump.contains("sqrtpd") && !dump.contains("div"), "{dump}");
    assert!(!dump.contains("call "), "{dump}");
    assert!(
        !dump.contains("stack_load") && !dump.contains("stack_store"),
        "{dump}"
    );
}

// The compiler never substitutes the estimate: the exact expression keeps
// its square root and division.
#[test]
#[cfg(target_arch = "x86_64")]
fn exact_reciprocal_square_root_is_not_rewritten_to_the_estimate() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("exact.plenty");
    std::fs::write(&input, "import std.simd as s\ndef kernel(a: &s.f32x4) -> s.f32x4:\n    s.splat_f32x4(1.0f32).div(a.sqrt())\ndef main() -> ():\n    a = s.splat_f32x4(4.0f32)\n    print(kernel(&a).to_tuple()).unwrap()\n").unwrap();
    let out = Command::new(support::plenty_bin())
        .env("PLENTY_CACHE", "off")
        .args(["--dump", "clif-opt,asm", "--dump-filter", "kernel"])
        .arg(input)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dump = String::from_utf8_lossy(&out.stdout);
    assert!(dump.contains("sqrtps") && dump.contains("divps"), "{dump}");
    assert!(!dump.contains("rsqrt"), "{dump}");
}

#[test]
#[cfg(target_arch = "x86_64")]
fn square_root_chains_stay_in_vector_registers() {
    for (vector, opcode) in [("f32x4", "sqrtps"), ("f64x2", "sqrtpd")] {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("sqrt.plenty");
        std::fs::write(&input, format!("import std.simd as s\ndef kernel(a: &s.{vector}, b: &s.{vector}) -> s.{vector}:\n    v = a.mul(b)\n    root = v.sqrt()\n    root.div(v)\ndef main() -> ():\n    a = s.splat_{vector}(4.0)\n    print(kernel(&a, &a).to_tuple()).unwrap()\n")).unwrap();
        let out = Command::new(support::plenty_bin())
            .env("PLENTY_CACHE", "off")
            .args(["--dump", "clif-opt,asm", "--dump-filter", "kernel"])
            .arg(input)
            .args(["-o", "-"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let dump = String::from_utf8_lossy(&out.stdout);
        assert!(dump.contains(opcode), "{dump}");
        assert!(!dump.contains("call "), "{dump}");
        assert!(
            !dump.contains("stack_load") && !dump.contains("stack_store"),
            "{dump}"
        );
    }
}

#[test]
#[cfg(target_arch = "x86_64")]
fn vector_projection_loop_avoids_storage_and_calls() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("projection.plenty");
    std::fs::write(
        &input,
        include_str!("../experiments/simd-boundaries/projection.plenty"),
    )
    .unwrap();
    let out = Command::new(support::plenty_bin())
        .env("PLENTY_CACHE", "off")
        .args(["--dump", "clif-opt,asm", "--dump-filter", "projection"])
        .arg(input)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dump = String::from_utf8_lossy(&out.stdout);
    assert!(dump.contains("addpd"), "{dump}");
    assert!(!dump.contains("call "), "{dump}");
    assert!(
        !dump.contains(" = load")
            && !dump.contains("    store")
            && !dump.contains("stack_store")
            && !dump.contains("stack_load"),
        "{dump}"
    );
}

#[rstest]
#[case(false)]
#[case(true)]
fn vector_tuple_copies_mutation_and_escapes(#[case] release: bool) {
    passes(
        r#"
import std.simd as s
from std.test import check

def escaped(a: f64, b: f64) -> (f64, f64):
    s.f64x2.Lanes(a, b).to_tuple()

def change(t: &mut (f64, f64)) -> ():
    match t:
        case (a, b):
            *a = 100.0
            *b = 200.0

def main() -> ():
    mut v = s.f64x2.Lanes(1.0, -0.0)
    mut tuple = v.to_tuple()
    saved = tuple
    change(&mut tuple)
    a, b = saved
    check(a == 1.0 and (1.0).copysign(b) == -1.0, "tuple snapshot").unwrap()
    check(tuple == (100.0, 200.0), "tuple mutation").unwrap()
    v = s.f64x2.Lanes(3.0, 4.0)
    check(saved == (1.0, 0.0), "vector reassignment").unwrap()
    for n in range(10):
        pair = if n % 2 == 0:
            v.to_tuple()
        else:
            escaped(5.0, 6.0)
        x, y = pair
        expected = if n % 2 == 0:
            7.0
        else:
            11.0
        check(x + y == expected, "tuple join").unwrap()
    ia, ib, ic, id = s.i32x4.Lanes(i32.MIN, i32.MAX, -1i32, 7i32).to_tuple()
    check((ia, ib, ic, id) == (i32.MIN, i32.MAX, -1i32, 7i32), "signed lanes").unwrap()
    ua, ub, uc, ud = s.u32x4.Lanes(0u32, u32.MAX, 1u32, 42u32).to_tuple()
    check((ua, ub, uc, ud) == (0u32, u32.MAX, 1u32, 42u32), "unsigned lanes").unwrap()
    fa, fb, fc, fd = s.f32x4.Lanes(-0.0f32, f32.NAN, f32.INF, 1e-40f32).to_tuple()
    check((1.0f32).copysign(fa) == -1.0f32 and fb != fb and fc == f32.INF and fd == 1e-40f32, "float lane bits").unwrap()
    print("ok").unwrap()
"#,
        release,
    );
}

/// Every shape against a scalar statement of the transfer contract: a whole
/// vector fits only when `0 <= offset <= len - lanes`, and nothing else moves.
#[rstest]
#[case(true)]
#[case(false)]
fn list_transfers_keep_scalar_bounds_at_every_offset(#[case] release: bool) {
    for (vector, ty, count, zero, one, next, mark) in [
        (
            "i32x4",
            "i32",
            4,
            "0i32",
            "1i32",
            "next.wrapping_add(3i32)",
            "-7i32",
        ),
        (
            "u32x4",
            "u32",
            4,
            "0u32",
            "1u32",
            "next.wrapping_add(3u32)",
            "77u32",
        ),
        (
            "f32x4",
            "f32",
            4,
            "0.0f32",
            "1.5f32",
            "next + 2.0f32",
            "-0.25f32",
        ),
        ("f64x2", "f64", 2, "0.0", "1.5", "next + 2.0", "-0.25"),
    ] {
        let lanes = (0..count)
            .map(|lane| format!("source[offset + {lane}]"))
            .collect::<Vec<_>>()
            .join(", ");
        passes(
            &format!(
                r#"import std.simd as s
from std.test import check

def filled(n: i64) -> Result[list[{ty}], AllocError]:
    mut values: list[{ty}] = [{zero} for i in range(n)]?
    mut next = {one}
    for i in range(n):
        values[i] = next
        next = {next}
    Ok(values)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    marker = s.splat_{vector}({mark})
    missing: Option[s.{vector}] = Nothing
    offsets = [-9223372036854775807, -5, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 9223372036854775804, 9223372036854775806, 9223372036854775807]?
    for n in range(10):
        source = filled(n)?
        for at in range(len(offsets)):
            offset = offsets[at]
            fits = offset >= 0 and offset <= n - {count}
            loaded = s.load_{vector}(&source, offset)
            mut target = filled(n)?
            check(marker.store(&mut target, offset) == fits, "store result").unwrap()
            check(len(target) == n, "length").unwrap()
            if fits:
                expected = s.{vector}.Lanes({lanes})
                check(loaded == Some(expected), "load").unwrap()
                check(s.load_{vector}(&source, offset).unwrap() == expected, "unwrap").unwrap()
                match loaded:
                    case Some(vector):
                        check(vector == expected, "match").unwrap()
                    case Nothing:
                        check(False, "missing").unwrap()
            else:
                check(loaded == missing, "invalid load").unwrap()
            for i in range(n):
                if fits and i >= offset and i < offset + {count}:
                    check(target[i] == {mark}, "stored lane").unwrap()
                else:
                    check(target[i] == source[i], "untouched element").unwrap()
    print("ok")?
    Ok(())
"#
            ),
            release,
        );
    }
}

/// A loaded `Option[Vector]` keeps value semantics as a local, across loop
/// iterations, in a collection, through a borrow and across calls.
#[rstest]
#[case(true)]
#[case(false)]
fn loaded_options_copy_escape_and_survive_loops(#[case] release: bool) {
    passes(
        r#"import std.simd as s
from std.test import check

def first(values: &list[f64], offset: i64) -> Option[s.f64x2]:
    s.load_f64x2(values, offset)

def lanes(value: Option[s.f64x2]) -> (f64, f64):
    match value:
        case Some(vector):
            vector.to_tuple()
        case Nothing:
            (-1.0, -1.0)

def borrowed(value: &Option[s.f64x2]) -> (f64, f64):
    lanes(*value)

def choose(values: &list[f64], flag: bool) -> Option[s.f64x2]:
    mut result: Option[s.f64x2] = Nothing
    if flag:
        result = s.load_f64x2(values, 1)
    else:
        result = Some(s.splat_f64x2(9.0))
    result

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut values = [1.0, 2.0, 3.0, 4.0]?
    mut previous: Option[s.f64x2] = Nothing
    mut held: Option[s.f64x2] = Nothing
    mut kept: list[Option[s.f64x2]] = []?
    for offset in range(-1, 5):
        current = s.load_f64x2(&values, offset)
        if offset == 0:
            check(lanes(previous) == (-1.0, -1.0), "first previous").unwrap()
            held = current
        if offset == 2:
            check(lanes(previous) == (2.0, 3.0), "previous survives the next load").unwrap()
            check(borrowed(&held) == (1.0, 2.0), "held value").unwrap()
        check((previous == current) == (offset == -1 or offset == 4), "equality").unwrap()
        kept.append(current)?
        previous = current
    check(len(kept) == 6, "kept").unwrap()
    check(lanes(kept[0]) == (-1.0, -1.0) and lanes(kept[1]) == (1.0, 2.0), "kept start").unwrap()
    check(lanes(kept[3]) == (3.0, 4.0) and lanes(kept[4]) == (-1.0, -1.0), "kept end").unwrap()
    check(lanes(first(&values, 2)) == (3.0, 4.0), "returned").unwrap()
    check(lanes(choose(&values, True)) == (2.0, 3.0), "assigned load").unwrap()
    check(lanes(choose(&values, False)) == (9.0, 9.0), "assigned constructor").unwrap()
    loaded = s.load_f64x2(&values, 1).unwrap()
    mut copy = loaded
    copy = copy.add(loaded)
    check(loaded.store(&mut values, 0), "store").unwrap()
    check(loaded.to_tuple() == (2.0, 3.0) and copy.to_tuple() == (4.0, 6.0), "copies").unwrap()
    check(lanes(held) == (1.0, 2.0), "a load does not alias the list").unwrap()
    values.pop()
    values.pop()
    check(lanes(s.load_f64x2(&values, 0)) == (2.0, 3.0), "shortened list").unwrap()
    check(lanes(s.load_f64x2(&values, 1)) == (-1.0, -1.0), "removed elements").unwrap()
    print("ok")?
    Ok(())
"#,
        release,
    );
}

#[test]
#[cfg(target_arch = "x86_64")]
fn list_transfers_are_packed_without_calls_or_enum_storage() {
    for (vector, load, kernel) in [
        (
            "f64x2",
            "movupd",
            "match s.load_f64x2(x, p):\n            case Some(a):\n                if not a.mul(a).store(out, p):\n                    return False\n            case Nothing:\n                return False",
        ),
        (
            "f64x2",
            "movupd",
            "a = s.load_f64x2(x, p).unwrap()\n        if not a.mul(a).store(out, p):\n            return False",
        ),
        (
            "f32x4",
            "movups",
            "a = s.load_f32x4(x, p).unwrap()\n        if not a.mul(a).store(out, p):\n            return False",
        ),
        (
            "i32x4",
            "movdqu",
            "a = s.load_i32x4(x, p).unwrap()\n        if not a.wrapping_add(a).store(out, p):\n            return False",
        ),
        (
            "u32x4",
            "movdqu",
            "a = s.load_u32x4(x, p).unwrap()\n        if not a.wrapping_add(a).store(out, p):\n            return False",
        ),
    ] {
        let lane = &vector[..3];
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("transfer.plenty");
        std::fs::write(&input, format!("import std.simd as s\ndef kernel(x: &list[{lane}], out: &mut list[{lane}], n: i64) -> bool:\n    for p in range(n):\n        {kernel}\n    True\ndef main() -> ():\n    pass\n")).unwrap();
        let out = Command::new(support::plenty_bin())
            .env("PLENTY_CACHE", "off")
            .args(["--dump", "clif-opt,asm", "--dump-filter", "kernel"])
            .arg(input)
            .args(["-o", "-"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let dump = String::from_utf8_lossy(&out.stdout);
        let native = vector.replace('u', "i");
        assert!(dump.contains(&format!("load.{native}")), "{dump}");
        assert_eq!(dump.matches(load).count(), 2, "{dump}");
        assert!(!dump.contains("call "), "{dump}");
        assert!(
            !dump.contains("stack_load") && !dump.contains("stack_store"),
            "{dump}"
        );
    }
}

/// Lane type, lane count, zero and a nonzero lane literal for each shape.
const VIEW_SHAPES: [(&str, &str, i64, &str, &str); 4] = [
    ("f64x2", "f64", 2, "0.0", "7.0"),
    ("f32x4", "f32", 4, "0.0f32", "7.0f32"),
    ("i32x4", "i32", 4, "0i32", "7i32"),
    ("u32x4", "u32", 4, "0u32", "7u32"),
];

fn view_source(template: &str, shape: (&str, &str, i64, &str, &str)) -> String {
    let (vector, ty, lanes, zero, seven) = shape;
    template
        .replace("VECTOR", vector)
        .replace("LANES", &lanes.to_string())
        .replace("ZERO", zero)
        .replace("SEVEN", seven)
        .replace("TYPE", ty)
}

#[rstest]
#[case(false)]
#[case(true)]
fn indexed_views_cover_whole_vectors_and_leave_the_tail(#[case] release: bool) {
    for shape in VIEW_SHAPES {
        passes(
            &view_source(
                r#"import std.simd as s
from std.test import check

def transfer(values: &[TYPE], out: &mut [TYPE]) -> ():
    for i in range(s.len_VECTOR(values)):
        s.get_VECTOR(values, i).set(out, i)
    for i in range(s.tail_VECTOR(values), len(values)):
        out[i] = values[i]

def through_lists(values: &list[TYPE], out: &mut list[TYPE]) -> ():
    for i in range(s.len_VECTOR(values)):
        s.get_VECTOR(values, i).set(out, i)

def cases() -> Result[(), AllocError | OverflowError[i64]]:
    for length in range(2 * LANES + 2):
        values: list[TYPE] = [TYPE(i + 1) for i in range(length)]?
        vectors = length // LANES
        tail = vectors * LANES
        check(s.len_VECTOR(&values) == vectors, "len").unwrap()
        check(s.tail_VECTOR(&values) == tail, "tail").unwrap()
        mut out: list[TYPE] = [ZERO for i in range(length)]?
        for i in range(vectors):
            vector = s.get_VECTOR(&values, i)
            check(vector == s.load_VECTOR(&values, i * LANES).unwrap(), "get").unwrap()
            vector.set(&mut out, i)
            check(out[i * LANES + LANES - 1] == values[i * LANES + LANES - 1], "set is visible").unwrap()
        for i in range(tail):
            check(out[i] == values[i], "covered element").unwrap()
        for i in range(tail, length):
            check(out[i] == ZERO, "tail untouched").unwrap()
        mut copied: list[TYPE] = [ZERO for i in range(length)]?
        transfer(&values, &mut copied)
        check(copied == values, "loop and scalar tail").unwrap()
        mut listed: list[TYPE] = [ZERO for i in range(length)]?
        through_lists(&values, &mut listed)
        check(listed == out, "list references").unwrap()
        if length > 0:
            rest = (length - 1) // LANES
            check(s.len_VECTOR(&values[1:]) == rest, "slice len").unwrap()
            check(s.tail_VECTOR(&values[1:]) == rest * LANES, "slice tail").unwrap()
            mut shifted: list[TYPE] = [ZERO for i in range(length)]?
            for i in range(rest):
                check(s.get_VECTOR(&values[1:], i) == s.load_VECTOR(&values, i * LANES + 1).unwrap(), "slice get").unwrap()
                s.splat_VECTOR(SEVEN).set(&mut shifted[1:], i)
            check(shifted[0] == ZERO, "before the slice").unwrap()
            for i in range(1, rest * LANES + 1):
                check(shifted[i] == SEVEN, "slice set").unwrap()
            for i in range(rest * LANES + 1, length):
                check(shifted[i] == ZERO, "slice tail").unwrap()
    Ok(())

def main() -> ():
    cases().unwrap()
    print("ok").unwrap()
"#,
                shape,
            ),
            release,
        );
    }
}

#[rstest]
#[case(false)]
#[case(true)]
fn indexed_view_accesses_trap_outside_the_whole_vectors(#[case] release: bool) {
    for shape in VIEW_SHAPES {
        let vector = shape.0;
        let source = view_source(
            r#"import std.simd as s

def total(values: &[TYPE], stop: i64) -> s.VECTOR:
    mut sum = s.splat_VECTOR(ZERO)
    for i in range(stop):
        sum = s.get_VECTOR(values, i)
    sum

def fill(values: &mut [TYPE], stop: i64) -> ():
    for i in range(stop):
        s.splat_VECTOR(SEVEN).set(values, i)

def main() -> Result[(), AllocError | IoError | ParseError | OverflowError[i64]]:
    argv = args()?
    which = i64.parse(&argv[1])?
    mut values: list[TYPE] = [ZERO for i in range(2 * LANES + 1)]?
    mut empty: list[TYPE] = []?
    vector = s.splat_VECTOR(SEVEN)
    if which == 0:
        drop(s.get_VECTOR(&values, -1))
    elif which == 1:
        drop(s.get_VECTOR(&values, 2))
    elif which == 2:
        drop(s.get_VECTOR(&values, i64.MAX))
    elif which == 3:
        drop(s.get_VECTOR(&values, i64.MIN))
    elif which == 4:
        drop(s.get_VECTOR(&empty, 0))
    elif which == 5:
        drop(s.get_VECTOR(&values[LANES:], 1))
    elif which == 6:
        drop(total(&values, 3))
    elif which == 7:
        vector.set(&mut values, -1)
    elif which == 8:
        vector.set(&mut values, 2)
    elif which == 9:
        vector.set(&mut values, i64.MAX)
    elif which == 10:
        vector.set(&mut values, i64.MIN)
    elif which == 11:
        vector.set(&mut empty, 0)
    elif which == 12:
        vector.set(&mut values[LANES:], 1)
    elif which == 13:
        fill(&mut values, 3)
    else:
        drop(total(&values, 2))
        fill(&mut values, 2)
        drop(s.get_VECTOR(&values, 1))
        vector.set(&mut values, 1)
        print(values[2 * LANES - 1] == SEVEN and values[2 * LANES] == ZERO)?
    Ok(())
"#,
            shape,
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        let executable = dir.path().join("program");
        std::fs::write(&path, &source).unwrap();
        let mut compile = Command::new(support::plenty_bin());
        compile.env("PLENTY_CACHE", "off");
        if !release {
            compile.arg("--fast-compile");
        }
        let compiled = compile
            .arg("--compile")
            .arg(&path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        // The call site named by the trap: the line holding `needle`.
        let line = |needle: &str| {
            1 + source
                .lines()
                .position(|line| line.contains(needle))
                .unwrap()
        };
        let get = format!("std.simd.get_{vector}: index is outside 0..len_{vector}(values)");
        let set = format!("{vector}.set: index is outside 0..len_{vector}(values)");
        let get_call = format!("s.get_{vector}(");
        let sites = [
            line(&format!("{get_call}&values, -1)")),
            line(&format!("{get_call}&values, 2)")),
            line(&format!("{get_call}&values, i64.MAX)")),
            line(&format!("{get_call}&values, i64.MIN)")),
            line(&format!("{get_call}&empty, 0)")),
            line(&format!("{get_call}&values[")),
            line(&format!("sum = {get_call}values, i)")),
            line("vector.set(&mut values, -1)"),
            line("vector.set(&mut values, 2)"),
            line("vector.set(&mut values, i64.MAX)"),
            line("vector.set(&mut values, i64.MIN)"),
            line("vector.set(&mut empty, 0)"),
            line("vector.set(&mut values["),
            line(".set(values, i)"),
        ];
        for (case, site) in sites.iter().enumerate() {
            let out = Command::new(&executable)
                .arg(case.to_string())
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&out.stderr);
            let message = if case < 7 { &get } else { &set };
            assert_eq!(out.status.code(), Some(1), "{vector} case {case}: {stderr}");
            assert!(out.stdout.is_empty(), "{vector} case {case}");
            assert!(
                stderr.starts_with("error: std/simd.plenty:")
                    && stderr.contains(&format!(
                        ": {message}\nnote: called from main.plenty:{site}:"
                    )),
                "{vector} case {case}: {stderr}"
            );
        }
        let out = Command::new(&executable).arg("14").output().unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, b"True\n", "{vector}");
    }
}

#[test]
fn a_live_view_rejects_resizing_its_list() {
    for (source, borrow) in [
        (
            "import std.simd as s\ndef main() -> Result[(), AllocError]:\n    mut xs = [1.0, 2.0, 3.0, 4.0]?\n    view: &mut [f64] = &mut xs\n    s.splat_f64x2(2.0).set(view, 0)\n    xs.append(5.0)?\n    s.splat_f64x2(3.0).set(view, 1)\n    Ok(())\n",
            "while `xs[:]` is exclusively borrowed",
        ),
        (
            "import std.simd as s\ndef main() -> Result[(), AllocError]:\n    mut xs = [1i32, 2i32, 3i32, 4i32]?\n    view: &[i32] = &xs\n    drop(s.get_i32x4(view, 0))\n    drop(xs.pop())\n    drop(s.get_i32x4(view, 0))\n    Ok(())\n",
            "while `xs[:]` is borrowed",
        ),
    ] {
        let output = run(source, true);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(
            stderr.contains("conflicting borrow: cannot modify or exclusively borrow `xs`")
                && stderr.contains(borrow),
            "{stderr}"
        );
    }
}

#[test]
#[cfg(target_arch = "x86_64")]
fn view_loop_is_packed_with_one_count_test_per_view() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("magnitude.plenty");
    std::fs::write(
        &input,
        include_str!("../experiments/simd-transfers/magnitude.plenty"),
    )
    .unwrap();
    let out = Command::new(support::plenty_bin())
        .env("PLENTY_CACHE", "off")
        .args(["--dump", "asm", "--dump-filter", "magnitude_views"])
        .arg(input)
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dump = String::from_utf8_lossy(&out.stdout);
    let lines = dump.lines().collect::<Vec<_>>();
    // Each packed square root sits in one straight-line run between two
    // branches. The versioned loop's run is a whole pass: every view's count
    // was tested before the loop, leaving the counter's compare.
    let fast = (0..lines.len())
        .filter(|&at| lines[at].contains("sqrtpd"))
        .any(|root| {
            let start = lines[..root]
                .iter()
                .rposition(|line| support::native_branch(line))
                .map_or(0, |at| at + 1);
            let end = root
                + lines[root..]
                    .iter()
                    .position(|line| support::native_branch(line))
                    .unwrap();
            let pass = lines[start..=end]
                .iter()
                .filter(|line| line.contains(":  "))
                .collect::<Vec<_>>();
            let count = |name: &str| pass.iter().filter(|line| line.contains(name)).count();
            count("movupd") == 4 && count("cmp") == 1 && pass.len() <= 17
        });
    assert!(fast, "{dump}");
}

#[rstest]
#[case(false)]
#[case(true)]
fn vector_locals_held_through_borrows_do_not_alias(#[case] release: bool) {
    passes(
        r#"import std.simd as simd
from std.test import check

def touch(v: &mut simd.f64x2) -> ():
    pass

def bump(v: &mut simd.f64x2) -> ():
    *v = v.add(simd.splat_f64x2(100.0))

def rotate(a: simd.f64x2, n: i64) -> (f64, f64, f64, f64):
    mut b = simd.splat_f64x2(0.0)
    mut c = simd.splat_f64x2(0.0)
    touch(&mut b)
    touch(&mut c)
    for i in range(n):
        c = b
        b = a.add(b)
    (x, y) = b.to_tuple()
    (z, w) = c.to_tuple()
    (x, y, z, w)

def keep(a: simd.f64x2, n: i64) -> Result[list[simd.f64x2], AllocError]:
    mut out: list[simd.f64x2] = []?
    mut b = simd.splat_f64x2(0.0)
    touch(&mut b)
    for i in range(n):
        b = a.add(b)
        held = b
        out.append(held)?
        bump(&mut b)
    Ok(out)

def main() -> ():
    one = simd.f64x2.Lanes(1.0, 2.0)
    check(rotate(one, 3) == (3.0, 6.0, 2.0, 4.0), "rotated copies").unwrap()
    kept = keep(one, 3).unwrap()
    check(kept[0].to_tuple() == (1.0, 2.0), "first copy").unwrap()
    check(kept[1].to_tuple() == (102.0, 104.0), "second copy").unwrap()
    check(kept[2].to_tuple() == (203.0, 206.0), "third copy").unwrap()
    mut p = simd.splat_f64x2(1.0)
    mut q = p
    touch(&mut p)
    touch(&mut q)
    for i in range(3):
        q = p
        p = p.mul(simd.splat_f64x2(2.0))
        r = q
        bump(&mut q)
        check(q.sub(r).to_tuple() == (100.0, 100.0), "copy before the bump").unwrap()
    check(p.to_tuple() == (8.0, 8.0) and q.to_tuple() == (104.0, 104.0), "final").unwrap()
    print("ok").unwrap()
"#,
        release,
    );
}
