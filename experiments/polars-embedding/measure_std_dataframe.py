#!/usr/bin/env python3
"""Bounded final-shape measurements; run after other builds/tests finish."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent


def run(command, env, cwd):
    start = time.perf_counter_ns()
    result = subprocess.run(command, env=env, cwd=cwd, text=True, capture_output=True, check=True)
    return result, time.perf_counter_ns() - start


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--engine", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cpus", required=True, help="four distinct physical cores, comma separated")
    args = parser.parse_args()
    compiler, engine = args.compiler.resolve(), args.engine.resolve()
    with tempfile.TemporaryDirectory(prefix="plenty-dataframe-final-") as directory:
        root = Path(directory)
        env = dict(os.environ, PLENTY_DATAFRAME_ENGINE=str(engine),
                   PLENTY_CACHE_DIR=str(root / "cache"), PLENTY_LINKER_THREADS="1")
        binary = root / "query"
        command = [str(compiler), "--release", "--compile", str(ROOT / "bench_std_dataframe.plenty"), "-o", str(binary)]
        cold = [run(command + ["--no-cache"], env, root)[1] for _ in range(5)]
        run(command, env, root)
        warm = [run(command, env, root)[1] for _ in range(5)]
        rows = []
        cpus = [int(cpu) for cpu in args.cpus.split(",")]
        assert len(cpus) == 4 and set(cpus) <= os.sched_getaffinity(0)
        topology = Path("/sys/devices/system/cpu")
        identities = {( (topology / f"cpu{cpu}/topology/physical_package_id").read_text(),
                        (topology / f"cpu{cpu}/topology/core_id").read_text()) for cpu in cpus}
        assert len(identities) == 4, "choose distinct physical cores, not SMT siblings"
        for size in [1024, 1000000]:
            for threads in [1, 4]:
                reps = 100
                selected = cpus[:threads]
                current = dict(env, SPIKE_ROWS=str(size), SPIKE_REPS=str(reps), POLARS_MAX_THREADS=str(threads))
                for _ in range(5):
                    result, wall = run(["taskset", "-c", ",".join(map(str, selected)), str(binary)], current, root)
                    fields = result.stdout.split()
                    expected = sum(2 * (i % 100) for i in range(size) if i % 100 > 50) * reps
                    assert float(fields[4]) == expected and int(fields[5]) == size, fields
                    rows.append(dict(rows=size, threads=threads, repetitions=reps, input_ns=int(fields[0]),
                                     import_ns=int(fields[1]), query_ns=int(fields[2]) / reps,
                                     export_ns=int(fields[3]), process_ns=wall, cpus=selected))
        result = dict(compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
                      engine_sha256=hashlib.sha256(engine.read_bytes()).hexdigest(),
                      engine_bytes=engine.stat().st_size, executable_bytes=binary.stat().st_size,
                      compile_no_cache_ns=cold, compile_warm_ns=warm, samples=rows)
        counter = subprocess.run(
            ["perf", "stat", "-x", ";", "-e", "instructions,cycles",
             "taskset", "-c", ",".join(map(str, cpus)), str(binary)],
            env=current, cwd=root, text=True, capture_output=True)
        result["whole_process_perf"] = dict(returncode=counter.returncode,
                                            stdout=counter.stdout, stderr=counter.stderr)
        args.out.write_text(json.dumps(result, indent=2) + "\n")
        print("compile medians ms", statistics.median(cold)/1e6, statistics.median(warm)/1e6)
        for size in [1024, 1000000]:
            for threads in [1, 4]:
                samples = [row for row in rows if row["rows"] == size and row["threads"] == threads]
                print(size, threads, {key: statistics.median(row[key] for row in samples)/1e6
                                     for key in ["input_ns", "import_ns", "query_ns", "export_ns"]})


if __name__ == "__main__":
    main()
