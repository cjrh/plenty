#!/usr/bin/env python3
"""Compare fused serial pipelines with loops using optimized native programs.

Run after building plenty: python3 scripts/bench_pipelines.py target/debug/plenty.
The compiler binary may be a debug build; --release selects native optimization.
Compilation is measured separately with the function cache disabled. Timings are
observations, not a timing-sensitive test or an assertion of the proposal's goal.
"""

import argparse
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


DECLARATIONS = """
def even(n: &i64) -> bool:
    *n % 2 == 0
def square(n: i64) -> i64:
    (n * n) % 1000
def add(a: i64, b: i64) -> i64:
    a + b
"""
LOOP = """    mut total = 0
    for n in range(stop):
        if n % 2 == 0:
            total += (n * n) % 1000
    total
"""
PIPELINE = """    iter(range(stop)).filter(even).map(square).fold(0, add)
"""
CLOSURE = """    iter(range(stop)).filter(def(n: &i64) -> bool: *n % 2 == 0).map(def(n: i64) -> i64: (n * n) % 1000).fold(0, def(a: i64, b: i64) -> i64: a + b)
"""
CAPTURED = """    parity = 2
    modulus = 1000
    iter(range(stop)).filter(def [parity](n: &i64) -> bool: *n % parity == 0).map(def [modulus](n: i64) -> i64: (n * n) % modulus).fold(0, add)
"""


def measured(command, env):
    start = time.perf_counter()
    result = subprocess.run(command, env=env, check=True, capture_output=True)
    return time.perf_counter() - start, result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--items", type=int, default=20_000_000)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--units", type=int, default=100)
    parser.add_argument("--cases", nargs="+", choices=["named", "closure", "captured"], default=["named", "closure", "captured"])
    args = parser.parse_args()
    if min(args.items, args.runs, args.units) < 1:
        parser.error("items, runs and units must be positive")
    compiler = str(args.compiler.resolve())
    env = dict(os.environ, PLENTY_CACHE="off")
    with tempfile.TemporaryDirectory(prefix="plenty-pipelines-") as directory:
        root = Path(directory)
        timings = {}
        expected = None
        cases = [("loop", LOOP)] + [(label, {"named": PIPELINE, "closure": CLOSURE, "captured": CAPTURED}[label]) for label in args.cases]
        executables = {}
        for label, body in cases:
            source = root / f"{label}.plenty"
            executable = root / label
            source.write_text(DECLARATIONS + "def work(stop: i64) -> i64:\n" + body
                              + f"def main() -> ():\n    print(work({args.items})).unwrap()\n")
            compile_time, _ = measured([compiler, "--release", "--compile", str(source), "-o", str(executable)], env)
            executables[label] = executable
            timings[label] = []
            print(f"{label}: compile={compile_time:.6f}s")
            source.write_text(DECLARATIONS + "".join(
                f"def work_{unit}(stop: i64) -> i64:\n{body}"
                for unit in range(args.units)
            ) + "def main() -> ():\n    print(work_0(10)).unwrap()\n")
            checking = [measured([compiler, "--check", str(source)], env)[0] for _ in range(args.runs)]
            print(f"{label}: cold check {args.units} functions median={statistics.median(checking):.6f}s")
        # Alternate traversal order to reduce bias from frequency/thermal drift.
        for run in range(args.runs):
            order = cases if run % 2 == 0 else list(reversed(cases))
            for label, _ in order:
                duration, stdout = measured([str(executables[label])], env)
                if expected is None:
                    expected = stdout
                if stdout != expected:
                    raise RuntimeError(f"{label} produced a different result")
                timings[label].append(duration)
        medians = {label: statistics.median(samples) for label, samples in timings.items()}
        for label, _ in cases:
            print(f"{label}: native median={medians[label]:.6f}s result={expected.decode().strip()}")
            print(f"{label}: native samples=" + ",".join(f"{sample:.6f}" for sample in timings[label]))
            if label != "loop":
                print(f"{label}/loop runtime ratio={medians[label] / medians['loop']:.3f}")


if __name__ == "__main__":
    main()
