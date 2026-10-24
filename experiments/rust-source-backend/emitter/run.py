#!/usr/bin/env python3
"""Check real checked-IR emission and measure normal rustc; no Cargo dependencies in outputs."""
import argparse
import json
import os
from pathlib import Path
import resource
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def run(argv, *, ok=True, env=None):
    start = time.perf_counter_ns()
    proc = subprocess.run(list(map(str, argv)), text=True, capture_output=True, env=env)
    elapsed = (time.perf_counter_ns() - start) / 1e6
    if ok and proc.returncode:
        raise RuntimeError(f"{argv}: exit {proc.returncode}\n{proc.stdout}\n{proc.stderr}")
    return proc, elapsed


def emit(args, source, dest, exports, helpers=False, companions=(), module_size=0, no_inline=False):
    command = [args.emitter, source, dest]
    for name in exports:
        command += ["--export", f"{name}={name}"]
    for item in companions:
        command += ["--companion", item]
    if helpers:
        command += ["--helpers"]
    if module_size:
        command += ["--module-size", str(module_size)]
    if no_inline:
        command += ["--no-inline"]
    proc, ms = run(command)
    return {**json.loads(proc.stdout), "process_ms": ms}


def rustc(source, dest, level=0, *, extra=(), ok=True):
    return run(["rustc", "--edition=2024", "-C", "panic=abort", "-C", f"opt-level={level}",
                "-C", "debuginfo=0", "-C", "embed-bitcode=no", *extra, source, "-o", dest], ok=ok)


def native(args, source, dest, release=False, cache="off"):
    return run([args.plenty, "--compile", *( ["--release"] if release else [] ), source, "-o", dest],
               env={**os.environ, "PLENTY_CACHE": cache})


def mapped_diagnostics(proc, rust_file):
    locations = json.loads(Path(f"{rust_file}.map.json").read_text())
    result = []
    for line in proc.stderr.splitlines():
        try:
            diagnostic = json.loads(line)
        except json.JSONDecodeError:
            continue
        for span in diagnostic.get("spans", []):
            if Path(span["file_name"]).resolve() == rust_file.resolve():
                key = str(span["line_start"])
                if key in locations:
                    result.append({"plenty": locations[key], "message": diagnostic["message"]})
    return result


def check(args, out):
    exports = ["arithmetic", "floors", "minimum_modulo", "widths", "loops", "branches",
               "short_circuit", "tail_test", "host_roundtrip", "void_test", "patterns", "wraps"]
    calls = [f"generated::{name}({7 if name == 'host_roundtrip' else ''})" for name in exports]
    expected = "22 -3282 0 235422 61 15 17 32 25 4 39 -108\n"
    checks = []
    for release in [False, True]:
        exe = out / f"native-{release}"
        native(args, HERE / "cases.plenty", exe, release)
        assert run([exe])[0].stdout == expected
        checks.append(f"native values release={release}")
    for helpers, module_size in [(False, 0), (True, 0), (True, 3)]:
        generated = out / "generated.rs"
        emit(args, HERE / "cases.plenty", generated, exports, helpers,
             ["host_adjust=crate::host::adjust"], module_size, module_size != 0)
        host = out / "host.rs"
        host.write_text('mod generated; mod host { pub fn adjust(x:i64)->i64 { x.checked_mul(3).unwrap() } }\n'
                        + 'fn main() { println!("' + ' '.join(['{}'] * len(calls)) + '",'
                        + ','.join(calls) + '); }\n')
        for level in [0, 3]:
            exe = out / f"rust-{helpers}-{level}"
            rustc(host, exe, level)
            assert run([exe])[0].stdout == expected
            checks.append(f"Rust roundtrip/control/widths helpers={helpers} modules={module_size} opt={level}")
        # A genuine rustc error is traced to the checked Plenty companion declaration.
        host.write_text('mod generated; mod host { pub fn adjust(_:bool)->i64 { 1 } } fn main() {}\n')
        proc, _ = rustc(host, out / "wrong", extra=["--error-format=json"], ok=False)
        mapped = mapped_diagnostics(proc, generated)
        assert proc.returncode and any("cases.plenty:" in m["plenty"] for m in mapped), proc.stderr
        checks.append(f"Rust companion type error mapped helpers={helpers}")

    traps = {
        "overflow": "9223372036854775807 + 1",
        "zero": "3 // 0",
        "division_overflow": "-9223372036854775808 // -1",
        "narrow_division_overflow": "i64(-128i8 // -1i8)",
        "narrow_overflow": "i64(127i8 + 1i8)",
        "modulo_zero": "3 % 0",
        "cast": "i64(u8(n))",
        "negative_unsigned_cast": "i64(u8(n - 257))",
    }
    for name, expression in traps.items():
        source = out / f"trap_{name}.plenty"
        source.write_text(f"def crash() -> i64:\n    n = 256\n    {expression}\n\ndef main() -> i32:\n    i32(crash())\n")
        for release in [False, True]:
            exe = out / "native-trap"
            native(args, source, exe, release)
            assert run([exe], ok=False)[0].returncode != 0
        for helpers in [False, True]:
            emit(args, source, out / "generated.rs", ["crash"], helpers)
            host = out / "trap.rs"
            host.write_text("mod generated; fn main(){ generated::crash(); }\n")
            for level in [0, 3]:
                rustc(host, out / "rust-trap", level)
                assert run([out / "rust-trap"], ok=False)[0].returncode != 0
        checks.append(f"fatal {name}, both emit shapes and optimization modes")

    rejected = {
        "float": ("def bad(x: f64) -> f64:\n    x * 2.0\n", "supports only"),
        "borrow": ("def bad(x: &i64) -> i64:\n    *x\n", "supports only"),
        "mutual": ("def bad(x: i64) -> i64:\n    if x == 0:\n        return 0\n    other(x - 1)\ndef other(x: i64) -> i64:\n    bad(x)\n", "mutual recursion"),
        "recursion": ("def bad(x: i64) -> i64:\n    if x == 0:\n        return 0\n    bad(x-1) + 1\n", "non-tail recursion"),
        "frontend": ("def bad() -> i64:\n    True\n", "expected i64"),
        "unused_bad": ("def unused() -> i64:\n    True\ndef bad() -> i64:\n    2\n", "expected i64"),
    }
    for name, (source, message) in rejected.items():
        file = out / f"reject_{name}.plenty"
        file.write_text(source)
        proc, _ = run([args.emitter, file, out / "reject.rs", "--export", "bad=bad"], ok=False)
        assert proc.returncode and message in proc.stderr, (name, proc.stderr)
        checks.append(f"rejected {name}")
    emit(args, HERE / "cases.plenty", out / "generated.rs", ["arithmetic"], True)
    proc, _ = run(["rustc", "--edition=2024", "--crate-type=lib", out / "generated.rs", "-o", out / "unwind.rlib"], ok=False)
    assert proc.returncode and "requires panic=abort" in proc.stderr
    checks.append("rejected unwinding Rust build")
    # Independent Python floor/modulo oracle over every nonzero i8 divisor.
    # MIN // -1 is a trap (covered above), whereas MIN % -1 must return zero.
    exhaustive = out / "exhaustive.plenty"
    exhaustive.write_text('''def quotient(a: i8, b: i8) -> i8:
    a // b
def remainder(a: i8, b: i8) -> i8:
    a % b
def main() -> Result[(), IoError]:
    mut a = -128
    while a < 128:
        mut b = -128
        while b < 128:
            if b != 0:
                q = 0 if a == -128 and b == -1 else i64(quotient(i8(a), i8(b)))
                print(q, remainder(i8(a), i8(b)))?
            b += 1
        a += 1
    Ok(())
''')
    expected_pairs = ''.join(f"{0 if a == -128 and b == -1 else a // b} {a % b}\n"
                             for a in range(-128, 128) for b in range(-128, 128) if b)
    for release in [False, True]:
        native(args, exhaustive, out / "native-exhaustive", release)
        assert run([out / "native-exhaustive"])[0].stdout == expected_pairs
    host = out / "exhaustive.rs"
    host.write_text('''mod generated;
fn main() {
    for a in -128i16..128 {
        for b in -128i16..128 {
            if b != 0 {
                let q = if a == -128 && b == -1 { 0 } else { generated::quotient(a as i8, b as i8) };
                println!("{} {}", q, generated::remainder(a as i8, b as i8));
            }
        }
    }
}
''')
    for helpers in [False, True]:
        emit(args, exhaustive, out / "generated.rs", ["quotient", "remainder"], helpers, module_size=1)
        for level in [0, 3]:
            rustc(host, out / "rust-exhaustive", level)
            assert run([out / "rust-exhaustive"])[0].stdout == expected_pairs
    checks.append("all 65280 nonzero-divisor i8 pairs against independent Python oracle")
    return checks


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--emitter", type=Path, default=ROOT / "target/release/examples/rust_source_spike")
    parser.add_argument("--plenty", type=Path, default=ROOT / "target/release/plenty")
    parser.add_argument("--output", type=Path, default=HERE / "target/checks")
    args = parser.parse_args()
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    args.output.mkdir(parents=True, exist_ok=True)
    checks = check(args, args.output)
    (args.output / "checks.json").write_text(json.dumps(checks, indent=2) + "\n")
    print(f"{len(checks)} checks passed")


if __name__ == "__main__":
    main()
