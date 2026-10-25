#!/usr/bin/env python3
"""Validate and measure the optional Polars bridge, using prebuilt Rust artifacts.

Run Cargo separately: it is deliberately absent from the Plenty compilation path.
Outputs live under --out; source files and checked-in evidence are never changed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parent


def run(command, *, env, cwd, check=True):
    start = time.perf_counter_ns()
    result = subprocess.run(command, env=env, cwd=cwd, text=True, capture_output=True)
    elapsed = time.perf_counter_ns() - start
    if check and result.returncode:
        raise RuntimeError(f"{command}\n{result.stdout}\n{result.stderr}")
    return result, elapsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--cargo-target", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cpus", default="2,4,6,8")
    parser.add_argument("--trials", type=int, default=7)
    parser.add_argument("--checks-only", action="store_true")
    args = parser.parse_args()
    compiler = args.compiler.resolve()
    target = args.cargo_target.resolve() / "release"
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PLENTY_CACHE_DIR=str(out / "cache"), PLENTY_LINKER_THREADS="1")
    env.pop("PLENTY_CACHE", None)
    library = out / "libplenty_polars_spike.so"
    shutil.copy2(target / library.name, library)
    # Resolve the shared library beside the application, not through a temporary
    # absolute DT_NEEDED entry or a user-wide LD_LIBRARY_PATH.
    extra = ["--link-arg", "-fuse-ld=mold", "--link-arg", "-Wl,--thread-count=1"]
    foreign = ["--link-arg", f"-L{out}", "--link-arg", "-lplenty_polars_spike",
               "--link-arg", "-Wl,-rpath,$ORIGIN"]
    build_times = {}
    names = ["example", "checks", "bench_polars", "bench_clist", "startup", "startup_plain"]
    for name in names:
        flags = extra + ([] if name in {"bench_clist", "startup_plain"} else foreign)
        command = [str(compiler), "--release", "--compile", str(ROOT / f"{name}.plenty"),
                   "-o", str(out / name), *flags]
        cold = run([*command, "--no-cache"], env=env, cwd=out)[1]
        run(command, env=env, cwd=out)
        warm = [run(command, env=env, cwd=out)[1] for _ in range(3)]
        build_times[name] = {"no_cache_ns": cold, "warm_cache_ns": warm,
                             "bytes": (out / name).stat().st_size}
    expected_example = "0 Option[f64].Some(160.0)\n1 Option[f64].Some(300.0)\n" * 2
    result, _ = run([str(out / "example")], env=env, cwd=out)
    assert result.stdout == expected_example, result.stdout
    result, _ = run([str(out / "checks")], env=env, cwd=out)
    assert result.stdout == "3 Option[f64].Nothing\nTrue Option[f64].Some(-0.0)\nFalse 1\nOption[f64].Some(12.0)\n3\n0\n", result.stdout
    # Private constructors and opaque pointers must stay behind the binding.
    bad = out / "private.plenty"
    bad.write_text("import dataframe as df\ndef main() -> ():\n    drop(df.DataFrame())\n")
    result, _ = run([str(compiler), "--check", str(bad), "--module-root", str(ROOT)], env=env, cwd=out, check=False)
    assert result.returncode != 0 and "private" in result.stderr, result.stderr
    report = {"platform": platform.platform(), "cpus": args.cpus,
              "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
              "polars": "0.55.2", "library_bytes": library.stat().st_size,
              "build_times": build_times, "checks": "passed", "measurements": []}
    if not args.checks_only:
        cpus = args.cpus.split(",")
        for threads in [1, min(4, len(cpus))]:
            affinity = ",".join(cpus[:threads])
            for rows, reps in [(1024, 500), (1_000_000, 20)]:
                for mode in ["count", "group"]:
                    case_env = dict(env, POLARS_MAX_THREADS=str(threads),
                                    SPIKE_ROWS=str(rows), SPIKE_REPS=str(reps), SPIKE_MODE=mode)
                    samples = {name: [] for name in ["polars", "rust", "clist"]}
                    commands = {"polars": out / "bench_polars", "rust": target / "direct",
                                "clist": out / "bench_clist"}
                    expected = sum(1 if mode == "count" else (i % 100) * 2
                                   for i in range(rows) if i % 100 > 50) * reps
                    # Rotate order between trials, avoiding systematic first-run bias.
                    for trial in range(args.trials):
                        order = list(commands)
                        order = order[trial % 3:] + order[:trial % 3]
                        for name in order:
                            command = ["taskset", "-c", affinity, str(commands[name])]
                            result, wall = run(command, env=case_env, cwd=out)
                            inputs, copied, query, checksum = map(float, result.stdout.split())
                            assert checksum == expected, (name, rows, mode, checksum, expected)
                            samples[name].append({"input_ns": inputs, "import_ns": copied,
                                                  "query_ns": query / reps, "process_ns": wall})
                    entry = {"rows": rows, "reps": reps, "mode": mode, "threads": threads,
                             "samples": samples,
                             "median": {name: {key: statistics.median(x[key] for x in values)
                                               for key in values[0]} for name, values in samples.items()}}
                    report["measurements"].append(entry)
                    print(rows, mode, threads, {k: round(v["query_ns"]) for k, v in entry["median"].items()}, flush=True)
        report["startup_ns"] = {}
        for name in ["startup", "startup_plain"]:
            command = ["taskset", "-c", cpus[0], str(out / name)]
            run(command, env=env, cwd=out)
            report["startup_ns"][name] = [run(command, env=env, cwd=out)[1] for _ in range(21)]
        perf_env = dict(env, POLARS_MAX_THREADS="1", SPIKE_ROWS="1000000", SPIKE_REPS="100", SPIKE_MODE="group")
        report["perf"] = {}
        if shutil.which("perf"):
            for name, binary in [("polars", out / "bench_polars"), ("clist", out / "bench_clist")]:
                result, _ = run(["perf", "stat", "-x", ";", "-e", "instructions,cycles", "taskset", "-c", cpus[0], str(binary)], env=perf_env, cwd=out, check=False)
                report["perf"][name] = {"status": result.returncode, "output": result.stderr}
    (out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Checks passed; evidence: {out / 'results.json'}")


if __name__ == "__main__":
    main()
