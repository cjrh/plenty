#!/usr/bin/env python3
"""Compare serial pipelines, pool.map/reduce_tree, and .par pipelines.

Run after building plenty, pinned to physical cores so the pool's workers do not
share them:

    taskset -c 0,2,4,6,8,10,12,14 python3 -B scripts/bench_par_pipelines.py target/release/plenty

Each program builds its input outside the timed region, times one terminal per
repetition with the monotonic clock, and prints the elapsed nanoseconds and a
checksum. Executables are interleaved across rounds and every variant of a case
must print the same checksum. Timings are observations, not a test.
"""

import argparse
import os
from pathlib import Path
import statistics
import subprocess
import tempfile

PRELUDE = """from std.fmt import FormatError
from std.time import monotonic_now

def cheap(x: i64) -> i64:
    x * 3 + 1
def heavy(x: i64) -> i64:
    mut h = x % 2147483648
    for i in range(100):
        h = (h * 1103515245 + 12345) % 2147483648
    h
def cheap_f(x: f64) -> f64:
    x * 1.5 + 1.0
def heavy_f(x: f64) -> f64:
    mut h = x
    for i in range(100):
        h = h * 0.999 + 1.0
    h
def keep_cheap(x: &i64) -> bool:
    *x % 3 == 0
def keep_heavy(x: &i64) -> bool:
    heavy(*x) % 3 == 0
def keep_cheap_i(x: i64) -> i64:
    1 if x % 3 == 0 else 0
def keep_heavy_i(x: i64) -> i64:
    1 if heavy(x) % 3 == 0 else 0
def add(a: i64, b: i64) -> i64:
    a + b
def add_f(a: f64, b: f64) -> f64:
    a + b
def ints(n: i64) -> Result[list[i64], AllocError]:
    mut xs: list[i64] = []?
    xs.reserve(n)?
    for i in range(n):
        xs.append(i)?
    Ok(xs)
def floats(n: i64) -> Result[list[f64], AllocError]:
    mut xs: list[f64] = []?
    xs.reserve(n)?
    for i in range(n):
        xs.append(f64(i))?
    Ok(xs)
def span(n: i64) -> Result[range[i64], AllocError]:
    Ok(range(n))
def total(ys: &list[i64]) -> i64:
    mut t = 0
    for y in ys:
        t += *y
    t
"""

# Each body consumes `xs` and binds `out`, an i64 or f64 checksum. A second line,
# outside the timed region, derives `out` from the collected `ys`.
CASES = {
    "map+collect": ("ints", {
        "serial": "ys: list[i64] = iter(xs).map({f}).collect()?\nout = total(&ys)",
        "pool": "ys = pool.map({f}, xs)?\nout = total(&ys)",
        "par": "ys = iter(xs).par(&pool).map({f}).collect()?\nout = total(&ys)",
    }),
    "filter+count": ("ints", {
        "serial": "out = iter(xs).filter(keep_{cost}).count()",
        "pool": "ys = pool.map(keep_{cost}_i, xs)?\nout = iter(ys).sum()",
        "par": "out = iter(xs).par(&pool).filter(keep_{cost}).count()?",
    }),
    "sum i64": ("ints", {
        "serial": "out = iter(xs).map({f}).sum()",
        "pool": "out = pool.reduce_tree(add, pool.map({f}, xs)?)?.unwrap_or(0)",
        "par": "out = iter(xs).par(&pool).map({f}).sum()?",
    }),
    # A range source needs no rows, which separates the cost of reading a
    # list's rows from the cost of the chunked work.
    "sum i64 range": ("span", {
        "serial": "out = iter(xs).map({f}).sum()",
        "pool": "out = pool.reduce_tree(add, pool.map({f}, xs)?)?.unwrap_or(0)",
        "par": "out = iter(xs).par(&pool).map({f}).sum()?",
    }),
    "sum f64": ("floats", {
        "serial": "out = iter(xs).map({f}_f).sum()",
        "pool": "out = pool.reduce_tree(add_f, pool.map({f}_f, xs)?)?.unwrap_or(0.0)",
        "par": "out = iter(xs).par(&pool).map({f}_f).sum()?",
    }),
    "reduce": ("ints", {
        "serial": "out = iter(xs).map({f}).reduce(add).unwrap_or(0)",
        "pool": "out = pool.reduce_tree(add, pool.map({f}, xs)?)?.unwrap_or(0)",
        "par": "out = iter(xs).par(&pool).map({f}).reduce(add)?.unwrap_or(0)",
    }),
}


def program(source, body, items, reps, workers):
    # Only the first statement is timed; a second one computes the checksum.
    timed, _, checksum = body.partition("\n")
    return PRELUDE + f"""
def main() -> Result[(), AllocError | IoError | PoolError | PoolMapError | FormatError]:
    with ThreadPoolExecutor({workers}, {2 * workers})? as pool:
        for rep in range({reps}):
            xs = {source}({items})?
            start = monotonic_now()
            {timed}
            elapsed = monotonic_now().saturating_duration_since(&start).as_nanoseconds()
            {checksum}
            print(f"{{elapsed}} {{out}}"?)?
    Ok(())
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--sizes", type=int, nargs="+", default=[1_000, 100_000, 10_000_000])
    parser.add_argument("--heavy-sizes", type=int, nargs="+", default=[1_000, 100_000, 1_000_000])
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--reps", type=int, default=5)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--cases", nargs="+", choices=list(CASES), default=list(CASES))
    args = parser.parse_args()
    compiler = str(args.compiler.resolve())
    env = dict(os.environ, PLENTY_CACHE="off")
    with tempfile.TemporaryDirectory(prefix="plenty-par-bench-") as directory:
        root = Path(directory)
        groups = []
        for case in args.cases:
            source, variants = CASES[case]
            for cost in ("cheap", "heavy"):
                for items in (args.sizes if cost == "cheap" else args.heavy_sizes):
                    executables = {}
                    for variant, body in variants.items():
                        name = f"{case}-{cost}-{items}-{variant}".replace(" ", "_").replace("+", "_")
                        path = root / f"{name}.plenty"
                        path.write_text(program(source, body.format(f=cost, cost=cost), items, args.reps, args.workers))
                        subprocess.run([compiler, "--release", "--compile", str(path), "-o", str(root / name)],
                                       env=env, check=True)
                        executables[variant] = root / name
                    groups.append((case, cost, items, executables))
        print("| Case | Callback | Items | Serial | pool | par | par/serial | par/pool |")
        print("|---|---|---:|---:|---:|---:|---:|---:|")
        for case, cost, items, executables in groups:
            samples = {variant: [] for variant in executables}
            checksums = set()
            for round in range(args.rounds):
                order = list(executables.items())
                if round % 2:
                    order.reverse()
                for variant, executable in order:
                    lines = subprocess.run([str(executable)], env=env, check=True,
                                           capture_output=True, text=True).stdout.split("\n")
                    for line in filter(None, lines[1:]):  # the first repetition warms up
                        elapsed, checksum = line.split(" ", 1)
                        samples[variant].append(int(elapsed) / 1e6)
                        checksums.add(checksum if source_is_int(case) else round_float(checksum))
            if len(checksums) != 1:
                raise RuntimeError(f"{case} {cost} {items}: checksums differ: {checksums}")
            median = {variant: statistics.median(values) for variant, values in samples.items()}
            print(f"| {case} | {cost} | {items:,} | {median['serial']:.3f} | {median['pool']:.3f} | "
                  f"{median['par']:.3f} | {median['par'] / median['serial']:.2f} | "
                  f"{median['par'] / median['pool']:.2f} |", flush=True)


def source_is_int(case):
    return CASES[case][0] != "floats"


def round_float(text):
    # Regrouped float sums differ in the last bits; compare to 9 significant digits.
    return f"{float(text):.9g}"


if __name__ == "__main__":
    main()
