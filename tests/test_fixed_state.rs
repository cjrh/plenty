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
