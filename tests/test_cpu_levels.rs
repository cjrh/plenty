//! CPU levels (`--cpu`, `[build] cpu`; issue #962): parsing and errors, the
//! startup-check trampoline, VEX encodings without implicit FMA at
//! x86-64-v3, identical float results at every level, `ymm` registers for
//! `f64x4` at x86-64-v3 only, and cache separation.
//! See book/src/design/toolchain/01-building/linking.md#cpu-levels.
mod support;
use std::path::Path;
use std::process::{Command, Output};

#[cfg(target_arch = "x86_64")]
const LEVELS: [&str; 3] = ["x86-64", "x86-64-v2", "x86-64-v3"];

/// Scalar and `std.simd` float arithmetic whose results change if any
/// `x * y + z` is fused, printed with round-trip precision.
const FLOATS: &str = "import std.simd as simd

def step(x: f64, y: f64, z: f64) -> f64:
    x * y + z

def main() -> Result[(), IoError]:
    mut x = 0.1
    mut v = simd.f64x2.Lanes(0.1, 0.7)
    scale = simd.splat_f64x2(1.0000001)
    offset = simd.splat_f64x2(0.3)
    mut w = simd.f32x4.Lanes(0.1f32, 0.2f32, 0.3f32, 0.4f32)
    mut u = simd.f64x4.Lanes(0.1, 0.7, -0.0, 2.5)
    limit = simd.splat_f64x4(1.0)
    for _ in range(1000):
        x = step(x, 1.0000001, 0.3) / 1.3
        v = v.mul(scale).add(offset).div(simd.splat_f64x2(1.3))
        w = w.mul(simd.splat_f32x4(1.0001f32)).add(simd.splat_f32x4(0.01f32)).sqrt()
        grown = u.mul(simd.splat_f64x4(1.0000001)).add(simd.splat_f64x4(0.3)).sqrt()
        u = grown.simd_gt(&limit).select(&grown.high().concat(&grown.low()), &grown)
    print(x)?
    print(v.to_tuple())?
    print(w.to_tuple())?
    print(u.to_tuple(), u.sum_ordered(0.0))?
    print(w.to_f64x4().to_tuple(), u.to_f32x4().to_f64x2().to_tuple(), v.to_f32x4().to_f64x2().to_tuple())?
    Ok(())
";

/// `std.simd.f64x4` results printed as bit patterns: NaN payloads (quiet,
/// signaling, negative), signed zeros, infinities and a subnormal through
/// every operation, list and view transfers, and a loop whose vectors cross
/// calls. At x86-64-v3 each `f64x4` is one `ymm` register.
const F64X4_BITS: &str = r#"import std.bytes
import std.encoding
import std.simd as s
from std.encoding import Endian

def from_bits(value: u64) -> f64:
    mut field = std.bytes.with_capacity(8).unwrap()
    std.encoding.append_u64(&mut field, value, Endian.Little).unwrap()
    std.encoding.unpack_f64(&field, 0, Endian.Little)

def show(label: str, v: &s.f64x4) -> Result[(), IoError]:
    mut field = std.bytes.with_capacity(32).unwrap()
    (a, b, c, d) = v.to_tuple()
    std.encoding.append_f64(&mut field, a, Endian.Big).unwrap()
    std.encoding.append_f64(&mut field, b, Endian.Big).unwrap()
    std.encoding.append_f64(&mut field, c, Endian.Big).unwrap()
    std.encoding.append_f64(&mut field, d, Endian.Big).unwrap()
    print(label, std.encoding.hex_text(&field).unwrap())

# The loop calls `print` while `a`, `b` and `c` are live, so each crosses a
# call in a spill slot.
def spilled(n: i64, x: s.f64x4, y: s.f64x4) -> Result[s.f64x4, IoError]:
    mut a = x
    mut b = y
    mut c = x.mul(y)
    for i in range(n):
        print(i)?
        a = a.add(b).div(c)
        b = b.mul(c).sqrt()
        c = c.sub(a)
    Ok(a.add(b).add(c))

# Narrowing rounds each lane to `f32`; widening it back is exact.
def converted(x: &s.f64x4) -> s.f64x4:
    x.to_f32x4().to_f64x4()

def streamed(values: &mut list[f64]) -> f64:
    mut total = s.splat_f64x4(0.0)
    for i in range(s.len_f64x4(values)):
        v = s.get_f64x4(values, i)
        total = total.add(v.mul(v)).div(v.sqrt())
        total.set(values, i)
    total.sum_ordered(0.0)

def main() -> Result[(), AllocError | IoError]:
    quiet = from_bits(0x7FF8000000001234u64)
    signaling = from_bits(0xFFF0000000000ABCu64)
    tiny = from_bits(1u64)
    specials = s.f64x4.Lanes(quiet, -0.0, f64.INF, -f64.INF)
    others = s.f64x4.Lanes(signaling, 0.0, -f64.INF, tiny)
    plain = s.f64x4.Lanes(0.1, -3.5, 1e300, 2.5e-308)
    vectors = [specials, others, plain, plain.high().concat(specials.low())]?
    for x in &vectors:
        show("sqrt", &x.sqrt())?
        show("f32", &converted(x))?
        for y in &vectors:
            show("add", &x.add(y))?
            show("sub", &x.sub(y))?
            show("mul", &x.mul(y))?
            show("div", &x.div(y))?
            show("select lt", &x.simd_lt(y).select(x, y))?
            show("select ge", &x.simd_ge(y).select(x, y))?
            show("select eq", &x.simd_eq(y).select(y, x))?
            show("select ne", &x.simd_ne(y).select(y, x))?
            show("select le", &x.simd_le(y).select(y, x))?
            show("select gt", &x.simd_gt(y).select(y, x))?
            print("masks", x.simd_lt(y).count(), x.simd_eq(y).any(), x.simd_ne(y).all())?
            show("halves", &x.high().concat(y.low()))?
        print("sum", x.sum_ordered(-0.0), x.lane(3))?
    show("spilled", &spilled(5, plain, others.add(plain))?)?
    mut values = [0.5, 2.0, quiet, -0.0, f64.INF, tiny, 3.0, 1e-300, 7.0]?
    print("streamed", streamed(&mut values))?
    loaded = s.load_f64x4(&values, 1).unwrap()
    show("loaded", &loaded)?
    print("stored", loaded.sqrt().store(&mut values, 5), loaded.store(&mut values, 6))?
    print(values)?
    Ok(())
"#;

/// The compiler with the suite's cache settings removed and the cache off,
/// unless a test sets `PLENTY_CACHE_DIR` itself.
fn plenty(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_plenty"));
    command
        .current_dir(dir)
        .env_remove("PLENTY_CACHE_DIR")
        .env_remove("PLENTY_CACHE_LIMIT")
        .env("PLENTY_CACHE", "off");
    command
}

fn succeed(output: Output, what: &str) -> Output {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn fail(output: Output) -> String {
    assert!(!output.status.success(), "unexpected success");
    String::from_utf8(output.stderr).unwrap()
}

fn setup(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("demo.plenty"), source).unwrap();
    dir
}

/// The object `plenty --emit-object` writes at `level`.
#[cfg(target_arch = "x86_64")]
fn object(dir: &Path, level: &str, configure: impl FnOnce(&mut Command)) -> Vec<u8> {
    let out = dir.join(format!("demo-{level}.o"));
    let mut command = plenty(dir);
    configure(&mut command);
    command.args(["--emit-object", "--cpu", level, "demo.plenty", "-o"]);
    succeed(command.arg(&out).output().unwrap(), level);
    std::fs::read(out).unwrap()
}

#[cfg(target_arch = "x86_64")]
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn unknown_missing_and_foreign_levels_are_rejected() {
    let dir = setup(FLOATS);
    let unknown = fail(
        plenty(dir.path())
            .args(["--cpu", "native", "demo.plenty"])
            .output()
            .unwrap(),
    );
    assert!(
        unknown.contains("unknown CPU level `native`; valid levels: x86-64, x86-64-v2, x86-64-v3"),
        "{unknown}"
    );
    let missing = fail(plenty(dir.path()).arg("--cpu").output().unwrap());
    assert!(
        missing.contains("--cpu requires a level: x86-64, x86-64-v2, x86-64-v3"),
        "{missing}"
    );
    let foreign = fail(
        plenty(dir.path())
            .args([
                "--target",
                "aarch64-unknown-linux-gnu",
                "--cpu",
                "x86-64-v2",
            ])
            .args(["--emit-object", "demo.plenty", "-o", "out.o"])
            .output()
            .unwrap(),
    );
    assert!(
        foreign.contains("CPU level `x86-64-v2` needs an x86-64 target"),
        "{foreign}"
    );
}

#[test]
fn print_target_names_a_non_baseline_level() {
    let dir = tempfile::tempdir().unwrap();
    let plain = succeed(
        plenty(dir.path()).arg("--print-target").output().unwrap(),
        "plain",
    );
    let triple = String::from_utf8(plain.stdout).unwrap();
    let native = triple.split(' ').next().unwrap();
    assert!(triple.contains("(CPU baseline: "), "{triple}");
    let baseline = succeed(
        plenty(dir.path())
            .args(["--print-target", "--cpu", "x86-64"])
            .output()
            .unwrap(),
        "baseline",
    );
    assert_eq!(String::from_utf8(baseline.stdout).unwrap(), triple);
    if cfg!(target_arch = "x86_64") {
        let v3 = succeed(
            plenty(dir.path())
                .args(["--print-target", "--cpu", "x86-64-v3"])
                .output()
                .unwrap(),
            "v3",
        );
        assert_eq!(
            String::from_utf8(v3.stdout).unwrap(),
            format!("{native} (CPU level: x86-64-v3)\n")
        );
    }
}

#[cfg(target_arch = "x86_64")]
#[test]
fn libraries_reject_levels_above_the_baseline() {
    let source = "@export(c)\ndef twice(x: i64) -> i64:\n    x * 2\n";
    let dir = setup(source);
    for mode in ["--shared-library", "--static-library"] {
        let stderr = fail(
            plenty(dir.path())
                .args([
                    mode,
                    "demo.plenty",
                    "--library-name",
                    "demo",
                    "--cpu",
                    "x86-64-v2",
                ])
                .args(["-o", "out"])
                .output()
                .unwrap(),
        );
        assert!(
            stderr.contains("CPU level `x86-64-v2` applies to executables"),
            "{mode}: {stderr}"
        );
    }
}

/// Only a non-baseline object has the trampoline: the baseline object
/// exports the program as `plenty_main` directly, as before CPU levels.
#[cfg(target_arch = "x86_64")]
#[test]
fn only_non_baseline_objects_check_the_cpu() {
    let dir = setup(FLOATS);
    let baseline = object(dir.path(), "x86-64", |_| {});
    let unflagged = {
        let out = dir.path().join("plain.o");
        succeed(
            plenty(dir.path())
                .args(["--emit-object", "demo.plenty", "-o"])
                .arg(&out)
                .output()
                .unwrap(),
            "plain",
        );
        std::fs::read(out).unwrap()
    };
    assert_eq!(baseline, unflagged, "`--cpu x86-64` changed the object");
    assert!(!contains(&baseline, b"plenty_rt_require_cpu"));
    assert!(!contains(&baseline, b"__plenty_main"));
    for level in ["x86-64-v2", "x86-64-v3"] {
        let checked = object(dir.path(), level, |_| {});
        assert!(contains(&checked, b"plenty_rt_require_cpu"), "{level}");
        assert!(contains(&checked, b"__plenty_main"), "{level}");
    }
}

/// Each level's executables print the same results, so no level fuses,
/// reassociates or otherwise changes float arithmetic. Run only on a CPU
/// that has each level; the startup check refuses the others.
#[cfg(target_arch = "x86_64")]
#[test]
fn every_level_prints_identical_float_results() {
    let dir = setup(FLOATS);
    let mut outputs = Vec::new();
    for level in LEVELS {
        if level == "x86-64-v3" && !std::is_x86_feature_detected!("avx2") {
            continue;
        }
        let exe = dir.path().join(format!("demo-{level}"));
        succeed(
            plenty(dir.path())
                .args(["--compile", "--cpu", level, "demo.plenty", "-o"])
                .arg(&exe)
                .output()
                .unwrap(),
            level,
        );
        let run = succeed(Command::new(&exe).output().unwrap(), level);
        outputs.push((level, String::from_utf8(run.stdout).unwrap()));
    }
    for (level, output) in &outputs[1..] {
        assert_eq!(output, &outputs[0].1, "{level} differs from x86-64");
    }
}

/// x86-64-v3 code uses VEX encodings and never contracts `x * y + z` into FMA;
/// the trampoline is compiled at the baseline.
#[cfg(target_arch = "x86_64")]
#[test]
fn v3_emits_vex_without_fma() {
    let dir = setup(FLOATS);
    for (level, vex) in [("x86-64", false), ("x86-64-v3", true)] {
        let out = dir.path().join(format!("dump-{level}"));
        succeed(
            plenty(dir.path())
                .args(["--dump", "asm", "--cpu", level, "demo.plenty", "-o"])
                .arg(&out)
                .output()
                .unwrap(),
            level,
        );
        let asm = std::fs::read_to_string(out.join("asm.txt")).unwrap();
        let step = asm
            .split("==== ")
            .find(|function| function.starts_with("step "))
            .unwrap_or_else(|| panic!("no step function in {asm}"));
        assert_eq!(step.contains("vmulsd"), vex, "{level}: {step}");
        assert_eq!(step.contains("vaddsd"), vex, "{level}: {step}");
        assert!(!asm.contains("vfmadd"), "{level} fused: {asm}");
        if vex {
            let entry = asm
                .split("==== ")
                .find(|function| function.contains("symbol=plenty_main "))
                .unwrap_or_else(|| panic!("no trampoline in {asm}"));
            assert!(entry.contains("plenty_rt_require_cpu"), "{entry}");
        }
    }
}

/// One cache directory serves every level without mixing their code: each
/// level's object equals its uncached object, in any order.
#[cfg(target_arch = "x86_64")]
#[test]
fn caches_keep_levels_apart() {
    let dir = setup(FLOATS);
    let cache = dir.path().join("cache");
    let uncached: Vec<_> = LEVELS
        .iter()
        .map(|level| object(dir.path(), level, |_| {}))
        .collect();
    for _ in 0..2 {
        for (level, expected) in LEVELS.iter().zip(&uncached).rev() {
            let cached = object(dir.path(), level, |command| {
                command
                    .env_remove("PLENTY_CACHE")
                    .env("PLENTY_CACHE_DIR", &cache);
            });
            assert!(cached == *expected, "{level} reused another level's code");
        }
    }
    assert!(uncached[0] != uncached[1] && uncached[1] != uncached[2]);
}

/// `[build] cpu` sets a package's level; `--cpu` overrides it.
#[cfg(target_arch = "x86_64")]
#[test]
fn manifest_level_applies_and_the_flag_overrides_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.plenty"), FLOATS).unwrap();
    let manifest = |cpu: &str| {
        std::fs::write(
            dir.path().join("plenty.toml"),
            format!("manifest-version = 1\n[package]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[build]\ncpu = \"{cpu}\"\n"),
        )
        .unwrap();
    };
    let build = |args: &[&str]| {
        let output = plenty(dir.path())
            .args(["build", "--no-strip"])
            .args(args)
            .output()
            .unwrap();
        let output = succeed(output, "plenty build");
        drop(output);
        std::fs::read(dir.path().join("target/optimized/demo")).unwrap()
    };
    manifest("x86-64-v2");
    assert!(contains(&build(&[]), b"plenty_rt_require_cpu"));
    assert!(!contains(
        &build(&["--cpu", "x86-64"]),
        b"plenty_rt_require_cpu"
    ));
    manifest("native");
    let stderr = fail(plenty(dir.path()).arg("build").output().unwrap());
    assert!(stderr.contains("unknown CPU level `native`"), "{stderr}");
    assert!(stderr.contains("plenty.toml"), "{stderr}");
}

/// Under an emulated CPU without the level, the startup check prints one line
/// and exits with status 1 before any program code runs; with the level, the
/// program runs. Skipped where `qemu-x86_64` is not installed.
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
#[test]
fn startup_check_refuses_cpus_without_the_level() {
    if Command::new("qemu-x86_64")
        .arg("-version")
        .output()
        .is_err()
    {
        eprintln!("skipped: qemu-x86_64 is not installed");
        return;
    }
    let dir = setup(FLOATS);
    let mut expected = None;
    for level in LEVELS {
        let exe = dir.path().join(format!("demo-{level}"));
        succeed(
            plenty(dir.path())
                .args(["--compile", "--cpu", level, "demo.plenty", "-o"])
                .arg(&exe)
                .output()
                .unwrap(),
            level,
        );
        for (cpu, passes) in [
            ("qemu64", level == "x86-64"),
            ("Nehalem", level != "x86-64-v3"),
            ("Haswell", true),
        ] {
            let run = Command::new("qemu-x86_64")
                .args(["-cpu", cpu])
                .arg(&exe)
                .output()
                .unwrap();
            let stderr = String::from_utf8_lossy(&run.stderr);
            if passes {
                let stdout = String::from_utf8(succeed(run, cpu).stdout).unwrap();
                assert_eq!(expected.get_or_insert_with(|| stdout.clone()), &stdout);
            } else {
                assert_eq!(run.status.code(), Some(1), "{level} on {cpu}: {stderr}");
                assert!(run.stdout.is_empty(), "{level} on {cpu} ran");
                assert_eq!(stderr.lines().count(), 1, "{stderr}");
                assert!(
                    stderr.starts_with(&format!(
                        "error: this program requires an {level} CPU (missing: "
                    )),
                    "{stderr}"
                );
            }
        }
    }
}

/// `plenty script` takes `--cpu` and `[build] cpu`, the flag first.
#[cfg(target_arch = "x86_64")]
#[test]
fn scripts_take_a_level() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("demo.plenty");
    std::fs::write(
        &script,
        format!("---plenty\n[build]\ncpu = \"x86-64-v2\"\n---\n{FLOATS}"),
    )
    .unwrap();
    let compile = |args: &[&str]| {
        let exe = dir.path().join("demo");
        succeed(
            plenty(dir.path())
                .args(["script", "--compile", "-o"])
                .arg(&exe)
                .args(args)
                .arg(&script)
                .output()
                .unwrap(),
            "plenty script",
        );
        std::fs::read(exe).unwrap()
    };
    let manifest = compile(&["--no-strip"]);
    assert!(contains(&manifest, b"plenty_rt_require_cpu"));
    let flag = compile(&["--no-strip", "--cpu", "x86-64"]);
    assert!(!contains(&flag, b"plenty_rt_require_cpu"));
}

/// `f64x4` gives the same bits at every level, optimized or not. The
/// unoptimized build runs the scalar Plenty bodies, so it is the reference.
#[cfg(target_arch = "x86_64")]
#[test]
fn f64x4_bits_match_at_every_level_and_mode() {
    let dir = setup(F64X4_BITS);
    let mut outputs = Vec::new();
    for level in LEVELS {
        if level == "x86-64-v3" && !std::is_x86_feature_detected!("avx2") {
            continue;
        }
        for mode in [None, Some("--fast-compile")] {
            let exe = dir.path().join(format!("demo-{level}-{}", mode.is_some()));
            succeed(
                plenty(dir.path())
                    .args(["--compile", "--cpu", level])
                    .args(mode)
                    .args(["demo.plenty", "-o"])
                    .arg(&exe)
                    .output()
                    .unwrap(),
                level,
            );
            let run = succeed(Command::new(&exe).output().unwrap(), level);
            outputs.push(((level, mode), String::from_utf8(run.stdout).unwrap()));
        }
    }
    let reference = &outputs[1].1;
    assert!(
        reference.contains("loaded 4006a09e667f3bcc7ff8000000001234fff8000000000000"),
        "{reference}"
    );
    for (build, output) in &outputs {
        assert_eq!(
            output, reference,
            "{build:?} differs from unoptimized x86-64"
        );
    }
}

/// Only x86-64-v3 code holds `f64x4` in `ymm` registers. A function with one
/// clears the upper halves before each call, and its spill slots hold all
/// 256 bits.
#[cfg(target_arch = "x86_64")]
#[test]
fn only_v3_uses_ymm_registers() {
    let dir = setup(F64X4_BITS);
    for level in LEVELS {
        let out = dir.path().join(format!("dump-{level}"));
        succeed(
            plenty(dir.path())
                .args(["--dump", "asm", "--cpu", level, "demo.plenty", "-o"])
                .arg(&out)
                .output()
                .unwrap(),
            level,
        );
        let asm = std::fs::read_to_string(out.join("asm.txt")).unwrap();
        let wide = level == "x86-64-v3";
        assert_eq!(asm.contains("%ymm"), wide, "{level}");
        let spilled = asm
            .split("==== ")
            .find(|function| function.starts_with("spilled "))
            .unwrap_or_else(|| panic!("no spilled function in {asm}"));
        if wide {
            let converted = asm
                .split("==== ")
                .find(|function| function.starts_with("converted "))
                .unwrap_or_else(|| panic!("no converted function in {asm}"));
            for op in ["vcvtpd2ps", "vcvtps2pd", "vinsertf128"] {
                assert!(converted.contains(op), "{op}: {converted}");
            }
            for op in ["vdivpd  %ymm", "vsqrtpd %ymm"] {
                assert!(spilled.contains(op), "{op}: {spilled}");
            }
            assert!(
                spilled.contains("(%rsp),%ymm"),
                "no 256-bit reload: {spilled}"
            );
            let instructions: Vec<_> = spilled
                .lines()
                .filter(|line| !line.trim_start().starts_with('#'))
                .collect();
            for pair in instructions.windows(2) {
                if pair[1].contains("  call ") {
                    assert!(pair[0].contains("vzeroupper"), "{spilled}");
                }
            }
        } else {
            assert!(!asm.contains("vzeroupper"), "{level}");
        }
    }
}

/// A function that only holds four `f64` values, as a 4-tuple or an
/// `f64x4` it passes on, keeps the two-part lowering at x86-64-v3: only a
/// function performing an `f64x4` operation, here `scale` and `main`, which
/// inlines it, uses `ymm` registers. `std.plot` and `examples/plot.plenty` hold such tuples and so
/// compile as at x86-64-v2.
#[cfg(target_arch = "x86_64")]
#[test]
fn only_f64x4_operations_make_a_function_wide() {
    let dir = setup(
        "import std.simd as s

def corners(x: f64, y: f64) -> (f64, f64, f64, f64):
    (x, y, x + 1.0, y + 1.0)

def summed(x: f64) -> f64:
    (p, q, r, t) = corners(x * 2.0, x + 3.0)
    p + q + r + t

def passed(v: s.f64x4) -> s.f64x4:
    v

def scale(x: f64) -> (f64, f64, f64, f64):
    passed(s.splat_f64x4(x)).mul(s.splat_f64x4(2.0)).to_tuple()

def main() -> Result[(), IoError]:
    print(summed(1.5))?
    print(scale(0.5))?
    Ok(())
",
    );
    let plot = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plot.plenty");
    std::fs::copy(plot, dir.path().join("plot.plenty")).unwrap();
    for source in ["demo.plenty", "plot.plenty"] {
        let out = dir.path().join(format!("dump-{source}"));
        succeed(
            plenty(dir.path())
                .args(["--dump", "asm", "--cpu", "x86-64-v3", source, "-o"])
                .arg(&out)
                .output()
                .unwrap(),
            source,
        );
        let asm = std::fs::read_to_string(out.join("asm.txt")).unwrap();
        for function in asm.split("==== ").skip(1) {
            let name = function.split_whitespace().next().unwrap();
            let wide = function.contains("%ymm") || function.contains("vzeroupper");
            let narrow = match source {
                "demo.plenty" => ["corners", "summed", "passed"].contains(&name),
                _ => !name.contains("f64x4"),
            };
            assert!(!(narrow && wide), "{source}: {function}");
            assert!(name != "scale" || wide, "{function}");
        }
    }
}

/// When the backend rejects a function's 256-bit form, the compiler builds
/// it again with two `f64x2` parts: the program prints the same results and
/// holds no `ymm` register. `PLENTY_TEST_REJECT_WIDE_SIMD` adds an
/// instruction the fork rejects to each function built wide. The function
/// and program caches keep the two forms apart in any order.
#[cfg(target_arch = "x86_64")]
#[test]
fn rejected_wide_functions_are_rebuilt_narrow() {
    const HOOK: &str = "PLENTY_TEST_REJECT_WIDE_SIMD";
    let dir = setup(F64X4_BITS);
    let asm = |rejected: bool| {
        let out = dir.path().join(format!("dump-{rejected}"));
        let mut command = plenty(dir.path());
        if rejected {
            command.env(HOOK, "1");
        }
        command.args(["--dump", "asm", "--cpu", "x86-64-v3", "demo.plenty", "-o"]);
        succeed(command.arg(&out).output().unwrap(), "dump");
        std::fs::read_to_string(out.join("asm.txt")).unwrap()
    };
    let wide = asm(false);
    assert!(wide.contains("%ymm") && !wide.contains("simd=rebuilt-narrow"));
    let narrow = asm(true);
    assert!(
        !narrow.contains("%ymm") && !narrow.contains("vzeroupper"),
        "{narrow}"
    );
    // `--dump` marks each rebuilt function, and only those.
    for function in wide.split("==== ").skip(1) {
        let name = function.split_whitespace().next().unwrap();
        let header = narrow
            .split("==== ")
            .find(|rebuilt| rebuilt.split_whitespace().next() == Some(name))
            .and_then(|rebuilt| rebuilt.lines().next())
            .unwrap_or_else(|| panic!("{name} missing from the rebuilt dump"));
        let was_wide = function.contains("%ymm") || function.contains("vzeroupper");
        assert_eq!(
            header.ends_with("simd=rebuilt-narrow"),
            was_wide,
            "{header}"
        );
    }
    if std::is_x86_feature_detected!("avx2") {
        let mut outputs = Vec::new();
        for rejected in [false, true] {
            let exe = dir.path().join(format!("demo-{rejected}"));
            let mut command = plenty(dir.path());
            if rejected {
                command.env(HOOK, "1");
            }
            command.args(["--compile", "--cpu", "x86-64-v3", "demo.plenty", "-o"]);
            succeed(command.arg(&exe).output().unwrap(), "compile");
            outputs.push(succeed(Command::new(&exe).output().unwrap(), "run").stdout);
        }
        assert_eq!(outputs[0], outputs[1]);
    }
    let level = "x86-64-v3";
    let wide = object(dir.path(), level, |_| {});
    let rebuilt = object(dir.path(), level, |command| {
        command.env(HOOK, "1");
    });
    assert!(wide != rebuilt);
    let cache = dir.path().join("cache");
    for _ in 0..3 {
        for (rejected, expected) in [(false, &wide), (true, &rebuilt)] {
            let cached = object(dir.path(), level, |command| {
                command
                    .env_remove("PLENTY_CACHE")
                    .env("PLENTY_CACHE_DIR", &cache);
                if rejected {
                    command.env(HOOK, "1");
                }
            });
            assert!(cached == *expected, "rejected {rejected}: mixed forms");
        }
    }
}
