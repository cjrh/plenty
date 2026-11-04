#!/usr/bin/env python3
"""Compare 64-value bitsets with 65-value hash sets using the same 64 values.

Build plenty first. Both variants compile with native --release optimization;
execute in alternating order and check results. The unused 65th variant forces
the ordinary hash-set representation, without changing the measured workload.
With --payload, use a six-boolean variant plus an optional unused extra case.
Timings are observations, not pass/fail performance assertions.
"""

import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import tempfile

from bench_pipelines import measured


def source(variants, workload, rounds, payload=False):
    declaration = "enum E:\n" + "".join(f"    V{i}\n" for i in range(variants))
    values = ", ".join(f"E.V{i}" for i in range(64))
    evens = ", ".join(f"E.V{i}" for i in range(0, 64, 2))
    odds = ", ".join(f"E.V{i}" for i in range(1, 64, 2))
    if payload:
        declaration = "enum E:\n    V(bool, bool, bool, bool, bool, bool)\n"
        if variants == 65:
            declaration += "    Extra\n"
        members = ["E.V(" + ", ".join("True" if n & (1 << bit) else "False" for bit in range(5, -1, -1)) + ")" for n in range(64)]
        values = ", ".join(members)
        evens = ", ".join(members[::2])
        odds = ", ".join(members[1::2])
    prefix = f"""
def main() -> Result[(), AllocError | IoError]:
    values = [{values}]?
    mut left = {{{evens}}}?
    right = {{{odds}}}?
    mut total = 0
"""
    bodies = {
        "lookup": """        for value in &values:
            if value in left:
                total += 1
""",
        "mutation": """        for value in &values:
            left.add(*value)?
        for value in &values:
            if left.discard(value):
                total += 1
""",
        "algebra": """        result = left.union(&right)?
        total += len(result)
""",
        "copy": """        result = copy(left)?
        total += len(result)
""",
    }
    return declaration + prefix + f"    for round in range({rounds}):\n" + bodies[workload] + "    print(total)?\n    Ok(())\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--rounds", type=int, default=200_000)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--payload", action="store_true", help="compare six-boolean payload domains of 64 and 65 values")
    args = parser.parse_args()
    if min(args.rounds, args.runs) < 1:
        parser.error("rounds and runs must be positive")
    compiler = str(args.compiler.resolve())
    env = dict(os.environ, PLENTY_CACHE="off")
    results = {"payload": args.payload, "machine": platform.machine(), "rounds": args.rounds, "runs": args.runs, "workloads": {}}
    with tempfile.TemporaryDirectory(prefix="plenty-enum-sets-") as directory:
        root = Path(directory)
        for workload in ("lookup", "mutation", "algebra", "copy"):
            binaries = {}
            times = {"bitset64": [], "hash65": []}
            expected = str(args.rounds * (64 if workload in ("mutation", "algebra") else 32)).encode()
            for name, count in (("bitset64", 64), ("hash65", 65)):
                path = root / f"{workload}-{name}.plenty"
                binary = path.with_suffix("")
                path.write_text(source(count, workload, args.rounds, args.payload))
                measured([compiler, "--release", "--compile", str(path), "-o", str(binary)], env)
                binaries[name] = binary
            for run in range(args.runs):
                order = ("bitset64", "hash65") if run % 2 == 0 else ("hash65", "bitset64")
                for name in order:
                    elapsed, output = measured([str(binaries[name])], env)
                    if output.strip() != expected:
                        raise RuntimeError(f"{workload}/{name}: unexpected output {output!r}")
                    times[name].append(elapsed)
            medians = {name: statistics.median(values) for name, values in times.items()}
            results["workloads"][workload] = {"seconds": times, "median_seconds": medians, "speedup": medians["hash65"] / medians["bitset64"]}
            print(f"{workload}: bitset={medians['bitset64']:.6f}s hash={medians['hash65']:.6f}s speedup={medians['hash65'] / medians['bitset64']:.2f}x", flush=True)
    if args.output:
        args.output.write_text(json.dumps(results, indent=2) + "\n")


if __name__ == "__main__":
    main()
