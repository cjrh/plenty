//! Statements lowered as two `f64x2` lanes store what the scalar code stores.
mod support;
#[cfg(target_arch = "x86_64")]
use std::path::Path;
use std::process::Command;

const NBODY: &str = include_str!("../benchmarks/game/n-body/plenty/nbody.plenty");
const NBODY_CLIST: &str = include_str!("../benchmarks/game/n-body/plenty/nbody_clist.plenty");

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

/// The `x` and `y` of a body, of its velocity and of a pair's difference are
/// rows: the differences subtract whole rows, and each pair of magnitudes
/// takes its two `x` lanes and its two `y` lanes with one shuffle each.
#[test]
#[cfg(target_arch = "x86_64")]
fn expanded_bodies_keep_x_and_y_in_one_vector() {
    for name in ["nbody.plenty", "nbody_clist.plenty"] {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("benchmarks/game/n-body/plenty")
            .join(name);
        let dump = |profile: &[&str]| {
            let out = support::assert_success(
                Command::new(support::plenty_bin())
                    .env("PLENTY_CACHE", "off")
                    .args(profile)
                    .args(["--dump", "clif-opt", "--dump-filter", "advance"])
                    .arg(&source)
                    .args(["-o", "-"])
                    .output()
                    .unwrap(),
            );
            String::from_utf8(out.stdout).unwrap()
        };
        let optimized = dump(&[]);
        assert_eq!(optimized.matches(" = shuffle ").count(), 10, "{optimized}");
        assert!(
            optimized.matches(" = fsub.f64x2 ").count() >= 10,
            "{optimized}"
        );
        // Rows are an optimization: the fast-compile profile has none.
        assert!(!dump(&["--fast-compile"]).contains("f64x2"));
    }
}

/// The program's output after 1, 2 and 25 steps: unoptimized, then optimized
/// at each CPU level this machine runs.
fn outputs(source: &str) -> Vec<[Vec<u8>; 3]> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("main.plenty");
    std::fs::write(&path, source).unwrap();
    let mut builds = vec![(false, plenty::CpuLevel::Baseline)];
    if cfg!(target_arch = "x86_64") {
        builds.push((true, plenty::CpuLevel::Baseline));
        builds.push((true, plenty::CpuLevel::X86_64V2));
        #[cfg(target_arch = "x86_64")]
        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
            builds.push((true, plenty::CpuLevel::X86_64V3));
        }
    } else {
        builds.push((true, plenty::CpuLevel::Baseline));
    }
    let mut outputs = Vec::new();
    for (index, (optimize, cpu)) in builds.into_iter().enumerate() {
        let program = dir.path().join(format!("build{index}"));
        plenty::compile_file_to_executable_with_options(
            &path,
            &program,
            None,
            &plenty::CompileOptions {
                optimize,
                cpu,
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
    outputs
}

/// `source` printing every field of every body after the final energy.
fn with_state(source: &str) -> String {
    let end = "    print(f\"{energy(&bodies)?:.9f}\"?)?\n    Ok(())";
    assert!(source.contains(end));
    source.replace(
        end,
        "    print(f\"{energy(&bodies)?:.9f}\"?)?\n    for i in range(len(bodies)):\n        print(f\"{bodies[i].x:.17e} {bodies[i].y:.17e} {bodies[i].z:.17e} {bodies[i].vx:.17e} {bodies[i].vy:.17e} {bodies[i].vz:.17e} {bodies[i].mass:.17e}\"?)?\n    Ok(())",
    )
}

/// Coincident bodies divide by zero, a distant one overflows the squared
/// distance and a close one makes it subnormal: each lane must store the
/// scalar instruction's infinity, NaN or rounded value.
#[test]
fn packed_lanes_match_unoptimized_code_for_special_values() {
    let jupiter = "4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,";
    let saturn = "8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,";
    let neptune = "1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,";
    for source in [NBODY, NBODY_CLIST] {
        let printed = with_state(source);
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
            let outputs = outputs(&printed.replace(from, to));
            for optimized in &outputs[1..] {
                assert_eq!(&outputs[0], optimized, "{to}");
            }
            let text = String::from_utf8_lossy(&outputs[1][2]);
            assert_eq!(text.lines().count(), 7, "{text}");
        }
    }
}

/// Two to four bodies pair their rows differently, and three leave a
/// magnitude with no partner: every count stores what the scalar code does.
#[test]
fn rows_match_unoptimized_code_for_every_body_count() {
    let printed = with_state(NBODY);
    for (count, first_removed) in [
        (4, "        # Neptune"),
        (3, "        # Uranus"),
        (2, "        # Saturn"),
    ] {
        let start = printed.find(first_removed).unwrap();
        let end = start + printed[start..].find("    ]\n").unwrap();
        let source = format!("{}{}", &printed[..start], &printed[end..]);
        let outputs = outputs(&source);
        for optimized in &outputs[1..] {
            assert_eq!(&outputs[0], optimized, "{count} bodies");
        }
        let text = String::from_utf8_lossy(&outputs[0][2]);
        assert_eq!(text.lines().count(), 2 + count, "{text}");
    }
}
