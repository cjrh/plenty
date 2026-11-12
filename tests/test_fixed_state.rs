//! Guarded scalar replacement must preserve complete state and fallback diagnostics.
mod support;
use std::path::Path;
use std::process::Command;

fn compile(source: &Path, output: &Path, optimize: bool) {
    plenty::compile_file_to_executable_with_options(
        source,
        output,
        None,
        &plenty::CompileOptions {
            optimize,
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
    let block = dump
        .split("\nblock")
        .find(|block| {
            block.lines().filter(|l| l.contains(" = fadd")).count() == 3
                && block.lines().filter(|l| l.contains(" = fmul")).count() == 3
                && block.lines().filter(|l| l.contains(" = fsub")).count() == 3
        })
        .unwrap_or_else(|| {
            panic!("three rows did not become one scalar arithmetic block:\n{dump}")
        });
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
const VECTORS: [(&str, &str, usize, &str); 2] =
    [("f64x2", "f64", 2, ""), ("f32x4", "f32", 4, "f32")];

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
            let (_dir, [slow, fast]) = both_profiles(&source);
            // `extra` rows make the lists longer than the guessed lengths:
            // the guard fails and the checked loop runs.
            for extra in [0, 1, 5] {
                for steps in [-1, 0, 1, 2, 9] {
                    let args = [steps.to_string(), extra.to_string()];
                    let a =
                        support::assert_success(Command::new(&slow).args(&args).output().unwrap());
                    let b =
                        support::assert_success(Command::new(&fast).args(&args).output().unwrap());
                    let case = format!("{} count {count} {args:?}", shape.0);
                    assert_eq!(a.stdout, b.stdout, "{case}");
                    let text = String::from_utf8(b.stdout).unwrap();
                    assert!(text.starts_with("True\n"), "{case}: {text}");
                    assert_eq!(text.lines().count(), 1 + count + extra, "{case}");
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
        let (_dir, programs) = both_profiles(&source);
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
        assert_eq!(passing[..3], passing[3..], "{vector}");
    }
}
