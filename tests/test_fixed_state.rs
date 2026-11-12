//! Guarded scalar replacement must preserve complete state and fallback diagnostics.
mod support;
use std::path::Path;
use std::process::Command;

fn compile(source: &Path, output: &Path, optimize: bool) {
    compile_at(source, output, optimize, plenty::CpuLevel::Baseline);
}

fn compile_at(source: &Path, output: &Path, optimize: bool, cpu: plenty::CpuLevel) {
    plenty::compile_file_to_executable_with_options(
        source,
        output,
        None,
        &plenty::CompileOptions {
            optimize,
            cpu,
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn guarded_state_matches_fallback_for_sizes_steps_and_bounds_failures() {
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fixed-state.plenty");
    let slow = dir.path().join("slow");
    let fast = dir.path().join("fast");
    compile(&source, &slow, false);
    compile(&source, &fast, true);
    for steps in [-1, 0, 1, 2, 17] {
        for extra in [0, 1, 3] {
            for size in [0, 2, 3, 6] {
                let args = [steps.to_string(), extra.to_string(), size.to_string()];
                let a = Command::new(&slow).args(&args).output().unwrap();
                let b = Command::new(&fast).args(&args).output().unwrap();
                assert_eq!(a.status, b.status, "{args:?}");
                assert_eq!(a.stdout, b.stdout, "{args:?}");
                assert_eq!(a.stderr, b.stderr, "{args:?}");
                assert_eq!(
                    b.status.success(),
                    steps <= 0 || size >= 3 + extra,
                    "{args:?}"
                );
            }
        }
    }
}

/// The clist fixture runs the same kernel over columns. A negative `extra`
/// leaves the clist without an allocation.
#[test]
fn guarded_clist_state_matches_fallback_for_sizes_steps_and_bounds_failures() {
    let dir = tempfile::tempdir().unwrap();
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fixed-state-clist.plenty");
    let slow = dir.path().join("slow");
    let fast = dir.path().join("fast");
    compile(&source, &slow, false);
    compile(&source, &fast, true);
    for steps in [-1, 0, 1, 2, 17] {
        for extra in [-1, 0, 1, 3] {
            for size in [0, 2, 3, 6] {
                let args = [steps.to_string(), extra.to_string(), size.to_string()];
                let a = Command::new(&slow).args(&args).output().unwrap();
                let b = Command::new(&fast).args(&args).output().unwrap();
                assert_eq!(a.status, b.status, "{args:?}");
                assert_eq!(a.stdout, b.stdout, "{args:?}");
                assert_eq!(a.stderr, b.stderr, "{args:?}");
                let rows = if extra < 0 { 0 } else { 3 + extra };
                assert_eq!(b.status.success(), steps <= 0 || size >= rows, "{args:?}");
            }
        }
    }
}

/// Runs both builds of `source` with each argument list and requires the
/// same status, output and diagnostics. Returns the optimized runs.
fn agree(source: &str, runs: &[Vec<i64>]) -> Vec<std::process::Output> {
    let (_dir, [slow, fast]) = both_profiles(source);
    runs.iter()
        .map(|run| {
            let args: Vec<String> = run.iter().map(i64::to_string).collect();
            let a = Command::new(&slow).args(&args).output().unwrap();
            let b = Command::new(&fast).args(&args).output().unwrap();
            assert_eq!(a.status, b.status, "{args:?}");
            assert_eq!(a.stdout, b.stdout, "{args:?}");
            assert_eq!(a.stderr, b.stderr, "{args:?}");
            b
        })
        .collect()
}

fn grid<const N: usize>(axes: [&[i64]; N]) -> Vec<Vec<i64>> {
    let mut runs = vec![vec![]];
    for axis in axes {
        runs = runs
            .iter()
            .flat_map(|run| axis.iter().map(|v| [&run[..], &[*v]].concat()))
            .collect();
    }
    runs
}

/// A borrowed clist and an owned one of `nb` rows; with no row the owned
/// clist has no allocation. Only three rows in each pass the guard.
#[test]
fn two_clist_roots_match_the_fallback_when_the_owned_one_is_unallocated() {
    let runs = grid([&[-1, 0, 1, 5], &[0, 2, 3, 4], &[0, 2, 3, 5]]);
    let outputs = agree(
        include_str!("fixtures/fixed-state-clist-pair.plenty"),
        &runs,
    );
    for (run, output) in runs.iter().zip(outputs) {
        let (steps, na, nb) = (run[0], run[1], run[2]);
        assert_eq!(
            output.status.success(),
            steps <= 0 || na == 0 || nb >= na,
            "{run:?}"
        );
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("clist index out of bounds"), "{run:?}");
        }
    }
}

/// Promoted `f64` and `f32` cells keep every bit: signalling and quiet NaN
/// payloads, a negative NaN, infinities and `-0.0` are loaded into locals,
/// permuted with sign changes and stored back.
#[test]
fn promoted_clist_cells_keep_nan_payloads_and_signed_zero() {
    let runs = grid([&[0, 1, 2, 3, 6, 7]]);
    let outputs = agree(
        include_str!("fixtures/fixed-state-clist-bits.plenty"),
        &runs,
    );
    let mut x = [
        0x7ff0_0000_0000_0001_u64,
        0x7ff8_dead_0000_beef,
        0xfff0_0000_0000_0005,
    ];
    let mut y = [0x8000_0000_u32, 0x7f80_0001, 0xffc0_ffee];
    let mut steps = 0;
    for (run, output) in runs.iter().zip(outputs) {
        // Each step runs the three-row body three times.
        for _ in 0..3 * (run[0] - steps) {
            x = [x[1] ^ (1 << 63), x[2], x[0]];
            y = [y[2], y[0], y[1] ^ (1 << 31)];
        }
        steps = run[0];
        let expected: String = (0..3)
            .map(|i| format!("{:016x}{:08x}", x[i], y[i]))
            .collect();
        let text = String::from_utf8(support::assert_success(output).stdout).unwrap();
        assert_eq!(text.trim(), expected, "steps {steps}");
    }
}

/// Rows of `bool, f32, u8, f64`: the float columns are cells and the other
/// columns keep their values.
#[test]
fn mixed_width_clist_rows_match_the_fallback() {
    let runs = grid([&[-1, 0, 1, 4], &[0, 2, 3, 5]]);
    let outputs = agree(
        include_str!("fixtures/fixed-state-clist-mixed.plenty"),
        &runs,
    );
    for (run, output) in runs.iter().zip(outputs) {
        let (steps, rows) = (run[0], run[1]);
        assert_eq!(output.status.success(), steps <= 0 || rows >= 3, "{run:?}");
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(text.lines().count() as i64, rows, "{run:?}");
        for (k, line) in text.lines().enumerate() {
            let fields: Vec<&str> = line.split(' ').collect();
            let flag = if k % 2 == 0 { "True" } else { "False" };
            assert_eq!((fields[0], fields[2]), (flag, &*k.to_string()), "{run:?}");
        }
    }
}

/// `xs[i] -= ps[j].x * dt` holds a deferred list cell across the store of
/// the clist read's index temporary.
#[test]
fn a_deferred_list_cell_held_across_a_local_store_matches_the_fallback() {
    let runs = grid([&[-1, 0, 1, 3, 11], &[0, 2, 3, 4]]);
    let outputs = agree(include_str!("fixtures/fixed-state-deferred.plenty"), &runs);
    for (run, output) in runs.iter().zip(outputs) {
        assert_eq!(
            output.status.success(),
            run[0] <= 0 || run[1] <= 3,
            "{run:?}"
        );
    }
}

#[test]
fn compound_assignments_across_two_list_roots_match_the_fallback() {
    let runs = grid([&[-1, 0, 1, 2, 7]]);
    for output in agree(include_str!("fixtures/fixed-state-list-pair.plenty"), &runs) {
        support::assert_success(output);
    }
}

/// A clist passed by value and moved into a mutable local is an owned root.
#[test]
fn a_clist_received_by_value_matches_the_fallback() {
    let runs = grid([&[-1, 0, 1, 5], &[0, 2, 3, 4]]);
    for (run, output) in runs.iter().zip(agree(
        include_str!("fixtures/fixed-state-clist-owned.plenty"),
        &runs,
    )) {
        let text = String::from_utf8(support::assert_success(output).stdout).unwrap();
        assert_eq!(text.lines().count() as i64, run[1], "{run:?}");
    }
}

/// Each operation that can change a clist's row count or move its columns
/// leaves the loop unpromoted (`clist_resizes_and_runtime_indices_decline_promotion`
/// in `src/codegen/fixed_state/tests.rs`); the builds must still agree,
/// including on the trap after rows are removed.
#[test]
fn clist_loops_that_resize_match_the_fallback() {
    let text = include_str!("fixtures/fixed-state-clist-resize.plenty");
    let anchor = "            values[i].y -= values[i].x\n";
    assert!(text.contains(anchor));
    let runs = grid([&[-1, 0, 1, 3], &[0, 2, 3, 4]]);
    for (line, traps) in [
        ("values.append(P(0.0, 0.0))?", false),
        ("values.reserve(1)?", false),
        ("drop(values.remove(0))", true),
        ("values.clear()", true),
        ("drop(values.copy_row(0))", false),
        ("drop(values[0:1]?)", false),
    ] {
        let source = text.replace(anchor, &format!("{anchor}        {line}\n"));
        let outputs = agree(&source, &runs);
        let trapped = runs.iter().zip(&outputs).any(|(run, output)| {
            run[0] == 3
                && run[1] == 3
                && String::from_utf8_lossy(&output.stderr).contains("clist index out of bounds")
        });
        assert_eq!(trapped, traps, "{line}");
    }
    for output in agree(text, &runs) {
        support::assert_success(output);
    }
}

#[test]
fn clist_nbody_matches_the_unoptimized_build_and_the_list_source() {
    let dir = tempfile::tempdir().unwrap();
    let rows = "    for i in range(len(bodies)):\n        print(f\"{bodies[i].x:.17e} {bodies[i].y:.17e} {bodies[i].z:.17e} {bodies[i].vx:.17e} {bodies[i].vy:.17e} {bodies[i].vz:.17e} {bodies[i].mass:.17e}\"?)?\n    Ok(())";
    let tail = "    print(f\"{energy(&bodies)?:.9f}\"?)?\n    Ok(())";
    let mut built = Vec::new();
    for (name, original) in [
        (
            "clist",
            include_str!("../benchmarks/game/n-body/plenty/nbody_clist.plenty"),
        ),
        (
            "list",
            include_str!("../benchmarks/game/n-body/plenty/nbody.plenty"),
        ),
    ] {
        assert!(original.contains(tail));
        let source = original.replace(tail, &tail.replace("    Ok(())", rows));
        let path = dir.path().join(format!("{name}.plenty"));
        std::fs::write(&path, source).unwrap();
        for optimize in [false, true] {
            let output = dir.path().join(format!("{name}-{optimize}"));
            compile(&path, &output, optimize);
            built.push(output);
        }
    }
    for steps in [-1, 0, 1, 2, 10, 1000, 10000] {
        let outputs: Vec<_> = built
            .iter()
            .map(|b| {
                support::assert_success(Command::new(b).arg(steps.to_string()).output().unwrap())
                    .stdout
            })
            .collect();
        assert_eq!(String::from_utf8_lossy(&outputs[0]).lines().count(), 7);
        for other in &outputs[1..] {
            assert_eq!(&outputs[0], other, "steps {steps}");
        }
    }
}

#[test]
fn original_nbody_preserves_all_fields_after_repeated_steps() {
    let dir = tempfile::tempdir().unwrap();
    let original = include_str!("../benchmarks/game/n-body/plenty/nbody.plenty");
    // Only add output: retain the original starting state, advance and callers.
    let source=original.replace("    print(f\"{energy(&bodies)?:.9f}\"?)?\n    Ok(())",
        "    print(f\"{energy(&bodies)?:.9f}\"?)?\n    for b in &bodies:\n        print(f\"{b.x:.17e} {b.y:.17e} {b.z:.17e} {b.vx:.17e} {b.vy:.17e} {b.vz:.17e} {b.mass:.17e}\"?)?\n    Ok(())");
    assert!(source.contains("{b.mass:.17e}"));
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let slow = dir.path().join("slow");
    let fast = dir.path().join("fast");
    compile(&path, &slow, false);
    compile(&path, &fast, true);
    for steps in [-1, 0, 1, 2, 10, 1000, 10000] {
        let a =
            support::assert_success(Command::new(&slow).arg(steps.to_string()).output().unwrap());
        let b =
            support::assert_success(Command::new(&fast).arg(steps.to_string()).output().unwrap());
        assert_eq!(a.stdout, b.stdout, "steps {steps}");
        assert_eq!(String::from_utf8_lossy(&b.stdout).lines().count(), 7);
    }
}

/// The magnitude loop of the benchmark through `std.simd` view accesses, as
/// `experiments/simd-transfers/measure.py` writes it.
const VIEW_MAGNITUDES: &str = "        xs: &[f64] = &dx
        ys: &[f64] = &dy
        zs: &[f64] = &dz
        out: &mut [f64] = &mut mag
        step = simd.splat_f64x2(dt)
        for pair in range(simd.len_f64x2(xs)):
            a = simd.get_f64x2(xs, pair)
            b = simd.get_f64x2(ys, pair)
            c = simd.get_f64x2(zs, pair)
            d2 = a.mul(a).add(b.mul(b)).add(c.mul(c))
            step.div(d2.mul(d2.sqrt())).set(out, pair)
        for p in range(simd.tail_f64x2(xs), len(xs)):
            d2 = xs[p] * xs[p] + ys[p] * ys[p] + zs[p] * zs[p]
            out[p] = dt / (d2 * d2.sqrt())
";

#[test]
fn nbody_with_vector_magnitudes_matches_the_scalar_source() {
    let dir = tempfile::tempdir().unwrap();
    let original = include_str!("../benchmarks/game/n-body/plenty/nbody.plenty");
    let scalar = original.replace("    print(f\"{energy(&bodies)?:.9f}\"?)?\n    Ok(())",
        "    print(f\"{energy(&bodies)?:.9f}\"?)?\n    for b in &bodies:\n        print(f\"{b.x:.17e} {b.y:.17e} {b.z:.17e} {b.vx:.17e} {b.vy:.17e} {b.vz:.17e} {b.mass:.17e}\"?)?\n    Ok(())");
    assert!(scalar.contains("{b.mass:.17e}"));
    let start = scalar.find("        for p in range(pairs):").unwrap();
    let end = start + scalar[start..].find("        k = 0").unwrap();
    let vector = format!("{}{VIEW_MAGNITUDES}{}", &scalar[..start], &scalar[end..]).replace(
        "import std.math",
        "import std.math\nimport std.simd as simd",
    );
    let mut programs = Vec::new();
    for (name, source, optimize) in [
        ("scalar", &scalar, true),
        ("slow", &vector, false),
        ("fast", &vector, true),
    ] {
        let path = dir.path().join(format!("{name}.plenty"));
        std::fs::write(&path, source).unwrap();
        let program = dir.path().join(name);
        compile(&path, &program, optimize);
        programs.push(program);
    }
    for steps in [-1, 0, 1, 2, 10, 1000, 10000] {
        let run = |program| {
            support::assert_success(
                Command::new(program)
                    .arg(steps.to_string())
                    .output()
                    .unwrap(),
            )
            .stdout
        };
        let expected = run(&programs[0]);
        assert_eq!(String::from_utf8_lossy(&expected).lines().count(), 7);
        assert_eq!(run(&programs[1]), expected, "unoptimized, steps {steps}");
        assert_eq!(run(&programs[2]), expected, "optimized, steps {steps}");
    }
}

#[cfg(feature = "runtime-checks")]
#[test]
fn scratch_allocation_failure_stays_before_the_specialized_loop() {
    let original = include_str!("fixtures/fixed-state.plenty");
    let source=original.replace("    update(&mut points, steps, scratch_size)?", "    print(\"__test_fail_allocations_after_0__\").unwrap()\n    result = update(&mut points, steps, scratch_size)\n    print(\"__test_restore_allocations__\").unwrap()\n    print(result.is_err())?");
    for optimize in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        let output = dir.path().join("program");
        std::fs::write(&path, &source).unwrap();
        plenty::compile_file_to_executable_with_options(
            &path,
            &output,
            None,
            &plenty::CompileOptions {
                optimize,
                test_hooks: true,
                ..Default::default()
            },
        )
        .unwrap();
        let out = support::assert_success(
            Command::new(output)
                .args(["10", "0", "3"])
                .output()
                .unwrap(),
        );
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.lines().any(|l| l == "True"), "{text}");
        assert!(text.contains("1.25000000000000000e+00"), "{text}");
    }
}

#[test]
fn expanded_float_updates_share_one_ssa_arithmetic_block() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fixed-state.plenty");
    let out = support::assert_success(
        Command::new(support::plenty_bin())
            .env("PLENTY_CACHE", "off")
            .args(["--dump", "clif-opt", "--dump-filter", "update"])
            .arg(source)
            .args(["-o", "-"])
            .output()
            .unwrap(),
    );
    let dump = String::from_utf8(out.stdout).unwrap();
    // On x86-64 the first two points are the lanes of three vector rows.
    let each = if cfg!(target_arch = "x86_64") { 2 } else { 3 };
    let block = dump
        .split("\nblock")
        .find(|block| {
            block.lines().filter(|l| l.contains(" = fadd")).count() == each
                && block.lines().filter(|l| l.contains(" = fmul")).count() == each
                && block.lines().filter(|l| l.contains(" = fsub")).count() == each
        })
        .unwrap_or_else(|| panic!("three rows did not become one arithmetic block:\n{dump}"));
    #[cfg(target_arch = "x86_64")]
    assert_eq!(block.matches(".f64x2 ").count(), 2, "{block}");
    assert!(
        !block.contains("load") && !block.contains("store") && !block.contains("call "),
        "{block}"
    );
    assert!(block.contains("brif"), "{block}");
}

#[test]
fn moved_owned_scratch_is_read_from_its_destination_slot() {
    let source = include_str!("fixtures/fixed-state.plenty")
        .replace("mut scratch =", "mut initial =")
        .replace("scratch.append", "initial.append")
        .replace(
            "    for _ in range(steps):",
            "    mut scratch = initial\n    for _ in range(steps):",
        );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let slow = dir.path().join("slow");
    let fast = dir.path().join("fast");
    compile(&path, &slow, false);
    compile(&path, &fast, true);
    for steps in [0, 1, 17] {
        let a = support::assert_success(
            Command::new(&slow)
                .args([steps.to_string(), "0".into(), "3".into()])
                .output()
                .unwrap(),
        );
        let b = support::assert_success(
            Command::new(&fast)
                .args([steps.to_string(), "0".into(), "3".into()])
                .output()
                .unwrap(),
        );
        assert_eq!(a.stdout, b.stdout);
    }
}

/// The `std.simd` shapes of the vector fixtures: vector, element, lanes and
/// a literal suffix.
const VECTORS: [(&str, &str, usize, &str); 3] = [
    ("f64x2", "f64", 2, ""),
    ("f64x4", "f64", 4, ""),
    ("f32x4", "f32", 4, "f32"),
];

fn vector_source(template: &str, shape: (&str, &str, usize, &str), count: usize) -> String {
    let (vector, ty, lanes, suffix) = shape;
    let literals = |n: usize| {
        (0..n)
            .map(|i| format!("{}.25{suffix}", i * 3 % 7))
            .collect::<Vec<_>>()
            .join(", ")
    };
    template
        .replace("VECTOR", vector)
        .replace("VALUES", &literals(count))
        .replace("HINT", &literals(2 * lanes))
        .replace("COUNT", &count.to_string())
        .replace("LANES", &lanes.to_string())
        .replace("LAST", &(2 * lanes - 1).to_string())
        .replace("ZERO", &format!("0.0{suffix}"))
        .replace("HALF", &format!("0.5{suffix}"))
        .replace("TYPE", ty)
}

fn both_profiles(source: &str) -> (tempfile::TempDir, [std::path::PathBuf; 2]) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let programs = [dir.path().join("slow"), dir.path().join("fast")];
    compile(&path, &programs[0], false);
    compile(&path, &programs[1], true);
    (dir, programs)
}

/// The scalar build, the optimized build and, where this machine runs it, the
/// optimized x86-64-v3 build, which holds `f64x4` in one `ymm` register.
fn vector_profiles(source: &str) -> (tempfile::TempDir, Vec<std::path::PathBuf>) {
    let (dir, programs) = both_profiles(source);
    let mut programs = programs.to_vec();
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        let wide = dir.path().join("wide");
        compile_at(
            &dir.path().join("main.plenty"),
            &wide,
            true,
            plenty::CpuLevel::X86_64V3,
        );
        programs.push(wide);
    }
    (dir, programs)
}

#[test]
fn vector_view_accesses_match_scalar_code_for_sizes_and_tails() {
    for shape in VECTORS {
        // Whole vectors only, every tail length, and fewer rows than a vector.
        for count in [3, 4, 5, 6, 7, 8] {
            let source = vector_source(
                include_str!("fixtures/fixed-state-simd.plenty"),
                shape,
                count,
            );
            let (_dir, programs) = vector_profiles(&source);
            let [slow, optimized @ ..] = &programs[..] else {
                unreachable!()
            };
            // `extra` rows make the lists longer than the guessed lengths:
            // the guard fails and the checked loop runs.
            for extra in [0, 1, 5] {
                for steps in [-1, 0, 1, 2, 9] {
                    let args = [steps.to_string(), extra.to_string()];
                    let a =
                        support::assert_success(Command::new(slow).args(&args).output().unwrap());
                    for fast in optimized {
                        let b = support::assert_success(
                            Command::new(fast).args(&args).output().unwrap(),
                        );
                        let case = format!("{} count {count} {args:?} {fast:?}", shape.0);
                        assert_eq!(a.stdout, b.stdout, "{case}");
                        let text = String::from_utf8(b.stdout).unwrap();
                        assert!(text.starts_with("True\n"), "{case}: {text}");
                        assert_eq!(text.lines().count(), 1 + count + extra, "{case}");
                    }
                }
            }
        }
    }
}

#[test]
fn vector_view_accesses_outside_a_short_list_trap_in_the_fallback() {
    for shape in VECTORS {
        let (vector, _, lanes, _) = shape;
        let source = vector_source(
            include_str!("fixtures/fixed-state-simd-bounds.plenty"),
            shape,
            0,
        );
        let line = |needle: &str| 1 + source.lines().position(|l| l.contains(needle)).unwrap();
        let get = format!(
            ": std.simd.get_{vector}: index is outside 0..len_{vector}(values)\nnote: called from main.plenty:{}:",
            line("s.get_")
        );
        let set = format!(
            ": {vector}.set: index is outside 0..len_{vector}(values)\nnote: called from main.plenty:{}:",
            line(".set(out, 1)")
        );
        let (_dir, programs) = vector_profiles(&source);
        let mut passing = Vec::new();
        for program in &programs {
            let run = |sources: usize, sinks: usize| {
                Command::new(program)
                    .args([sources.to_string(), sinks.to_string()])
                    .output()
                    .unwrap()
            };
            // Two vectors exactly pass the guard; longer lists and lists of
            // different lengths run the checked loop.
            for (sources, sinks) in [
                (2 * lanes, 2 * lanes),
                (2 * lanes + 3, 2 * lanes + 3),
                (2 * lanes + 1, 2 * lanes),
            ] {
                passing.push(support::assert_success(run(sources, sinks)).stdout);
            }
            for (sources, sinks, message) in [
                (2 * lanes - 1, 2 * lanes, &get),
                (0, 2 * lanes, &get),
                (2 * lanes, 2 * lanes - 1, &set),
                (2 * lanes + 3, lanes, &set),
            ] {
                let out = run(sources, sinks);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let case = format!("{vector} {sources} {sinks}: {stderr}");
                assert_eq!(out.status.code(), Some(1), "{case}");
                assert!(out.stdout.is_empty(), "{case}");
                assert!(stderr.contains(message.as_str()), "{case}");
            }
        }
        for program in passing.chunks(3).skip(1) {
            assert_eq!(passing[..3], *program, "{vector}");
        }
    }
}
