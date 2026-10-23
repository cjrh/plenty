#!/usr/bin/env python3
"""Measure local fluent consumption and explicit generator conversion.

Build plenty first. Native --release optimization is independent of the compiler
binary's build profile. All variants check the same result; alternate execution
order to reduce frequency/thermal bias. Timings are observations, not tests.
"""

import argparse
import os
from pathlib import Path
import statistics
import tempfile

from bench_pipelines import DECLARATIONS, LOOP, PIPELINE, measured


GENERATORS = """
def numbers(stop: i64) -> Generator[i64]:
    for n in range(stop):
        yield n
"""
CASES = {
    "inline_range": LOOP,
    "local_range": PIPELINE,
    "inline_generator": LOOP.replace("range(stop)", "numbers(stop)"),
    "wrapped_generator": "    iter(numbers(stop)).filter(even).map(square).fold(0, add)\n",
    "fluent_generator": "    numbers(stop).filter(even).map(square).fold(0, add)\n",
    "converted_range": "    iter(range(stop)).filter(even).map(square).generator().fold(0, add)\n",
    "converted_generator": "    numbers(stop).filter(even).map(square).generator().fold(0, add)\n",
    "legacy": "    legacy.fold(legacy.map(legacy.filter(numbers(stop), even), square), 0, add)\n",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--items", type=int, default=5_000_000)
    parser.add_argument("--runs", type=int, default=7)
    args = parser.parse_args()
    if min(args.items, args.runs) < 1:
        parser.error("items and runs must be positive")
    compiler = str(args.compiler.resolve())
    env = dict(os.environ, PLENTY_CACHE="off")
    with tempfile.TemporaryDirectory(prefix="plenty-generator-bridge-") as directory:
        root = Path(directory)
        timings = {label: [] for label in CASES}
        binaries = {}
        for label, body in CASES.items():
            source = root / f"{label}.plenty"
            binary = root / label
            imports = "import std.iter as legacy\n" if label == "legacy" else ""
            source.write_text(imports + DECLARATIONS + GENERATORS
                              + "def work(stop: i64) -> i64:\n" + body
                              + f"def main() -> ():\n    print(work({args.items})).unwrap()\n")
            elapsed, _ = measured([compiler, "--release", "--compile", str(source), "-o", str(binary)], env)
            print(f"{label}: compile={elapsed:.6f}s", flush=True)
            binaries[label] = binary
        expected = None
        for run in range(args.runs):
            order = list(CASES) if run % 2 == 0 else list(reversed(CASES))
            for label in order:
                elapsed, output = measured([str(binaries[label])], env)
                if expected is None:
                    expected = output
                if output != expected:
                    raise RuntimeError(f"{label} produced a different result: {output!r}")
                timings[label].append(elapsed)
        medians = {label: statistics.median(samples) for label, samples in timings.items()}
        for label, samples in timings.items():
            print(f"{label}: median={medians[label]:.6f}s samples="
                  + ",".join(f"{sample:.6f}" for sample in samples))
        for case, baseline in [("local_range", "inline_range"),
                               ("fluent_generator", "wrapped_generator"),
                               ("fluent_generator", "inline_generator"),
                               ("converted_range", "local_range"),
                               ("converted_range", "legacy"),
                               ("converted_generator", "legacy")]:
            print(f"{case}/{baseline}={medians[case] / medians[baseline]:.3f}")
        print(f"all results={expected.decode().strip()}")


if __name__ == "__main__":
    main()
