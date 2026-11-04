#!/usr/bin/env python3
"""Compare finite enum bitsets with hash-backed domains on identical workloads.

The unused variants extend the hash domain to 257 values. Native programs use
--release; runs alternate order and check results. --members controls density,
with values spread across the full domain to exercise every mask word.
--baseline-compiler instead compares identical sources against an older compiler.
JSON includes elapsed time and, where available, child user+system CPU time;
background load can still affect clock speeds and cache behavior.
"""

import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import tempfile

from bench_pipelines import measured

try:
    import resource
except ImportError:
    resource = None


def child_cpu_seconds():
    if resource is None:
        return 0.0
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    return usage.ru_utime + usage.ru_stime


def source(size, workload, rounds, payload=False, hashed=False, members=None):
    count = size if members is None else members
    indices = [n * (size - 1) // (count - 1) for n in range(count)]
    domain = 257 if hashed else size
    declaration = "enum E:\n" + "".join(f"    V{i}\n" for i in range(domain))
    values = [f"E.V{i}" for i in indices]
    if payload:
        bits = size.bit_length() - 1
        declaration = "enum E:\n    V(" + ", ".join(["bool"] * bits) + ")\n"
        declaration += "".join(f"    Extra{i}\n" for i in range(domain - size))
        values = ["E.V(" + ", ".join("True" if n & (1 << bit) else "False" for bit in range(bits - 1, -1, -1)) + ")" for n in indices]
    evens = ", ".join(values[::2])
    odds = ", ".join(values[1::2])
    values = ", ".join(values)
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
    parser.add_argument("--payload", action="store_true", help="use boolean payloads rather than payload-free variants")
    parser.add_argument("--size", type=int, choices=(64, 128, 256), default=64)
    parser.add_argument("--members", type=int, help="even number of workload values, spread across the domain")
    parser.add_argument("--baseline-compiler", type=Path, help="compare identical domains against another compiler instead of forcing hash storage")
    parser.add_argument("--workloads", nargs="+", choices=("lookup", "mutation", "algebra", "copy"), default=("lookup", "mutation", "algebra", "copy"))
    args = parser.parse_args()
    members = args.size if args.members is None else args.members
    if members < 2 or members > args.size or members % 2:
        parser.error("members must be even and between 2 and size")
    if min(args.rounds, args.runs) < 1:
        parser.error("rounds and runs must be positive")
    compiler = str(args.compiler.resolve())
    env = dict(os.environ, PLENTY_CACHE="off")
    comparison = "baseline" if args.baseline_compiler else "hash"
    results = {"payload": args.payload, "domain": args.size, "members": members, "comparison": comparison, "comparison_domain": args.size if args.baseline_compiler else 257, "machine": platform.machine(), "rounds": args.rounds, "runs": args.runs, "workloads": {}}
    with tempfile.TemporaryDirectory(prefix="plenty-enum-sets-") as directory:
        root = Path(directory)
        for workload in args.workloads:
            binaries = {}
            times = {"bitset": [], comparison: []}
            cpu_times = {"bitset": [], comparison: []}
            expected = str(args.rounds * (members if workload in ("mutation", "algebra") else members // 2)).encode()
            for name in ("bitset", comparison):
                path = root / f"{workload}-{name}.plenty"
                binary = path.with_suffix("")
                path.write_text(source(args.size, workload, args.rounds, args.payload, name == "hash", members))
                selected_compiler = str(args.baseline_compiler.resolve()) if name == "baseline" else compiler
                measured([selected_compiler, "--release", "--compile", str(path), "-o", str(binary)], env)
                binaries[name] = binary
            for run in range(args.runs):
                order = ("bitset", comparison) if run % 2 == 0 else (comparison, "bitset")
                for name in order:
                    cpu_start = child_cpu_seconds()
                    elapsed, output = measured([str(binaries[name])], env)
                    cpu_times[name].append(child_cpu_seconds() - cpu_start)
                    if output.strip() != expected:
                        raise RuntimeError(f"{workload}/{name}: unexpected output {output!r}")
                    times[name].append(elapsed)
            medians = {name: statistics.median(values) for name, values in times.items()}
            results["workloads"][workload] = {"seconds": times, "median_seconds": medians, "speedup": medians[comparison] / medians["bitset"]}
            if resource is not None:
                cpu_medians = {name: statistics.median(values) for name, values in cpu_times.items()}
                results["workloads"][workload].update(cpu_seconds=cpu_times, median_cpu_seconds=cpu_medians, cpu_speedup=cpu_medians[comparison] / cpu_medians["bitset"])
            print(f"{workload}: bitset={medians['bitset']:.6f}s {comparison}={medians[comparison]:.6f}s speedup={medians[comparison] / medians['bitset']:.2f}x", flush=True)
    if args.output:
        args.output.write_text(json.dumps(results, indent=2) + "\n")


if __name__ == "__main__":
    main()
