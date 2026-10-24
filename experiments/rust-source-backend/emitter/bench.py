#!/usr/bin/env python3
"""Repeatable artifact-cold and incremental rustc timing, kept separate from correctness checks."""
import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import time

from run import ROOT, HERE, emit, native, run, rustc

VARIANTS = {
    "inline-dev": (False, 0, 16, 0, False),
    "helpers-dev": (True, 0, 16, 0, False),
    "helpers-dev-one-unit": (True, 0, 1, 0, False),
    "helpers-opt1": (True, 1, 16, 0, False),
    "helpers-release": (True, 3, 16, 0, False),
    "helpers-dev-groups": (True, 0, 16, 32, False),
    "helpers-release-groups": (True, 3, 16, 32, False),
    "helpers-release-noinline": (True, 3, 16, 0, True),
    "helpers-release-groups-noinline": (True, 3, 16, 32, True),
}


def frequencies():
    result = {}
    for cpu in os.sched_getaffinity(0):
        path = Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq")
        if path.exists():
            result[str(cpu)] = int(path.read_text())
    return result


def program(functions, repetitions=100):
    pieces = []
    for i in range(functions):
        pieces.append(f"def kernel_{i}(x: i64) -> i64:\n")
        pieces.append(f"    mut value = x + {i + 1}\n")
        for j in range(12):
            pieces.append(f"    value = (value * 3 + {i + j + 1}) % 1000003\n")
        pieces.append("    value\n\n")
    pieces.append("def run(seed: i64, count: i64) -> i64:\n    mut n = 0\n    mut result = 0\n    while n < count:\n")
    for i in range(functions):
        pieces.append(f"        result = (result + kernel_{i}(seed + n)) % 1000003\n")
    pieces.append("        n += 1\n    result\n\ndef main() -> i32:\n")
    pieces.append(f"    i32(run(7, {repetitions}) % 200)\n")
    return "".join(pieces)


def sample(fn, repeats):
    times = []
    for _ in range(repeats):
        _, elapsed = fn()
        times.append(elapsed)
    return {"median_ms": statistics.median(times), "samples_ms": times}


def timing(args, count, out):
    out.mkdir(parents=True, exist_ok=True)
    source = out / "workload.plenty"
    source.write_text(program(count, args.iterations))
    generated = out / "generated.rs"
    host = out / "host.rs"
    # Separate library prevents whole-program constant propagation of the fixed seed.
    host.write_text(f"fn main() {{ std::process::exit((generated::run(7, {args.iterations}) % 200) as i32); }}\n")
    result = {"functions": count, "iterations": args.iterations, "plenty_bytes": source.stat().st_size, "variants": {}}
    for release in [False, True]:
        binary = out / f"plenty-{release}"
        result[f"plenty_{'release' if release else 'dev'}_compile_link"] = sample(
            lambda: native(args, source, binary, release), args.repeats)
        proc, _ = run([binary], ok=False)
        assert 0 <= proc.returncode < 200, proc.stderr
        result.setdefault("expected_status", proc.returncode)
        assert result["expected_status"] == proc.returncode
        result[f"plenty_{'release' if release else 'dev'}_run"] = sample(
            lambda: run([binary], ok=False), args.repeats)
    for shape in args.variants:
        frequency_start = frequencies()
        helpers, level, units, module_size, no_inline = VARIANTS[shape]
        emission = [emit(args, source, generated, ["run"], helpers, module_size=module_size, no_inline=no_inline) for _ in range(args.repeats)]
        library = out / "libgenerated.rlib"
        binary = out / f"rust-{shape}"
        flags = ["--crate-name", "generated", "--crate-type", "rlib", "-C", f"codegen-units={units}"]
        # No incremental cache: these are new compiler processes rebuilding artifacts,
        # with the OS page cache warm. Rust sysroot is already installed/prebuilt.
        build = sample(lambda: rustc(generated, library, level, extra=flags), args.repeats)
        link = sample(lambda: rustc(host, binary, level, extra=["--extern", f"generated={library}"]), args.repeats)
        proc, _ = run([binary], ok=False)
        assert proc.returncode == result["expected_status"], (shape, proc.returncode, result)
        execute = sample(lambda: run([binary], ok=False), args.repeats)
        check = sample(lambda: rustc(generated, out / "generated.rmeta", level,
                                    extra=[*flags, "--emit=metadata"]), args.repeats)
        result["variants"][shape] = {
            "helpers": helpers, "opt_level": level, "codegen_units": units, "module_size": module_size, "no_inline": no_inline,
            "emission_samples": emission, "library_compile": build, "host_compile_link": link,
            "metadata_only": check, "run": execute, "rlib_bytes": library.stat().st_size,
            "binary_bytes": binary.stat().st_size,
            "frequency_start_khz": frequency_start,
        }
        def complete_build():
            start = time.perf_counter_ns()
            emit(args, source, generated, ["run"], helpers, module_size=module_size, no_inline=no_inline)
            rustc(generated, library, level, extra=flags)
            proc, _ = rustc(host, binary, level, extra=["--extern", f"generated={library}"])
            return proc, (time.perf_counter_ns() - start) / 1e6

        if args.end_to_end:
            result["variants"][shape]["end_to_end"] = sample(complete_build, args.repeats)
            assert run([binary], ok=False)[0].returncode == result["expected_status"]
        result["variants"][shape]["frequency_end_khz"] = frequencies()
        (out / "partial.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"{count} functions: {shape} library {build['median_ms']:.1f} ms", flush=True)
    # Explicit compiler incrementality, including a real body edit in one function.
    module_size = 32 if any(VARIANTS[v][3] for v in args.variants) else 0
    emit(args, source, generated, ["run"], True, module_size=module_size)
    cache = out / f"incremental-{time.time_ns()}"
    library = out / "libincremental.rlib"
    flags = ["--crate-name", "generated", "--crate-type", "rlib", "-C", "codegen-units=16", "-C", f"incremental={cache}"]
    _, first = rustc(generated, library, extra=flags)
    unchanged = sample(lambda: rustc(generated, library, extra=flags), args.repeats)
    original = source.read_text()
    source.write_text(original.replace("mut value = x + 1\n", "mut value = x + 2\n", 1))
    edit_emission = emit(args, source, generated, ["run"], True, module_size=module_size)
    _, edit = rustc(generated, library, extra=flags)
    binary = out / "edited"
    _, edited_link = rustc(host, binary, extra=["--extern", f"generated={library}"])
    native(args, source, out / "native-edited")
    assert run([binary], ok=False)[0].returncode == run([out / "native-edited"], ok=False)[0].returncode
    result["incremental_dev"] = {"module_size": module_size, "first_ms": first, "unchanged": unchanged, "body_edit_ms": edit,
                                  "body_edit_emission": edit_emission, "edited_host_compile_link_ms": edited_link}
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--emitter", type=Path, default=ROOT / "target/release/examples/rust_source_spike")
    parser.add_argument("--plenty", type=Path, default=ROOT / "target/release/plenty")
    parser.add_argument("--output", type=Path, default=HERE / "target/bench")
    parser.add_argument("--sizes", type=int, nargs="+", default=[1, 100, 1000])
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--iterations", type=int, default=100)
    parser.add_argument("--variants", nargs="+", choices=list(VARIANTS), default=list(VARIANTS))
    parser.add_argument("--end-to-end", action="store_true", help="also time the entire generation/library/host pipeline together")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    results = {"rustc": run(["rustc", "-vV"])[0].stdout, "platform": platform.platform(),
               "affinity": sorted(os.sched_getaffinity(0)), "repeats": args.repeats,
               "flags": "edition=2024 panic=abort debuginfo=0 embed-bitcode=no; default installed linker; no external crates",
               "cache_note": "artifact-cold unless incremental_dev; OS pages warm; prebuilt installed sysroot",
               "workloads": []}
    results["cpu"] = json.loads(run(["lscpu", "--json"])[0].stdout)
    for size in args.sizes:
        result = timing(args, size, args.output / str(size))
        results["workloads"].append(result)
        (args.output / "measurements.json").write_text(json.dumps(results, indent=2) + "\n")
        print(f"{size} functions measured", flush=True)


if __name__ == "__main__":
    main()
