//! Statements lowered as two `f64x2` lanes store what the scalar code stores.
mod support;
#[cfg(target_arch = "x86_64")]
use std::path::Path;
use std::process::Command;

const NBODY: &str = include_str!("../benchmarks/game/n-body/plenty/nbody.plenty");

#[test]
#[cfg(target_arch = "x86_64")]
fn expanded_magnitudes_divide_and_take_roots_in_pairs() {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/game/n-body/plenty/nbody.plenty");
    let out = support::assert_success(
        Command::new(support::plenty_bin())
            .env("PLENTY_CACHE", "off")
            .args(["--dump", "clif-opt", "--dump-filter", "advance"])
            .arg(source)
            .args(["-o", "-"])
            .output()
            .unwrap(),
    );
    let dump = String::from_utf8(out.stdout).unwrap();
    // Ten pairs of bodies: five vector divisions in the specialized loop.
    assert_eq!(dump.matches(" = fdiv.f64x2 ").count(), 5, "{dump}");
}

/// Coincident bodies divide by zero, a distant one overflows the squared
/// distance and a close one makes it subnormal: each lane must store the
/// scalar instruction's infinity, NaN or rounded value.
#[test]
fn packed_lanes_match_unoptimized_code_for_special_values() {
    let printed = NBODY.replace("    print(f\"{energy(&bodies)?:.9f}\"?)?\n    Ok(())",
        "    print(f\"{energy(&bodies)?:.9f}\"?)?\n    for b in &bodies:\n        print(f\"{b.x:.17e} {b.y:.17e} {b.z:.17e} {b.vx:.17e} {b.vy:.17e} {b.vz:.17e} {b.mass:.17e}\"?)?\n    Ok(())");
    assert!(printed.contains("{b.mass:.17e}"));
    let jupiter = "4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,";
    let saturn = "8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,";
    let neptune = "1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,";
    for (from, to) in [
        (jupiter, "0.0, 0.0, 0.0,"),
        (saturn, "1.0e200, -1.0e200, 0.0,"),
        (neptune, "1.0e-160, 2.0e-161, -0.0,"),
        (
            saturn,
            "4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,",
        ),
    ] {
        assert!(printed.contains(from));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.plenty");
        std::fs::write(&path, printed.replace(from, to)).unwrap();
        let mut outputs = Vec::new();
        for optimize in [false, true] {
            let program = dir.path().join(if optimize { "fast" } else { "slow" });
            plenty::compile_file_to_executable_with_options(
                &path,
                &program,
                None,
                &plenty::CompileOptions {
                    optimize,
                    ..Default::default()
                },
            )
            .unwrap();
            let run = |steps: i64| {
                support::assert_success(
                    Command::new(&program)
                        .arg(steps.to_string())
                        .output()
                        .unwrap(),
                )
                .stdout
            };
            outputs.push([run(1), run(2), run(25)]);
        }
        assert_eq!(outputs[0], outputs[1], "{to}");
        let text = String::from_utf8_lossy(&outputs[1][2]);
        assert_eq!(text.lines().count(), 7, "{text}");
    }
}
