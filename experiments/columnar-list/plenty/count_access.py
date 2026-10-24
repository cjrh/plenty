#!/usr/bin/env python3
"""Count user-mode instructions for the native indexed/copy-row kernels.

Uses the same kernels as bench.py, but runs one repeatedly without timer/print
calls between iterations. Counts include startup, construction and teardown;
the zero-repeat control reports that overhead separately, without subtraction.
Linux perf access is required. Run bench.py separately for real timings.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess

from bench import expected, program
from generate import generate

HERE = Path(__file__).resolve().parent


def source(rows, repeats, kernel):
    declarations = program(rows, 1, 1, native=True, skip_generated=True).split("def main()", 1)[0]
    return declarations + f"""def main() -> Result[(), AllocError | AppendError[Particle] | IoError]:
    mut items = clist[Particle].with_capacity({rows})?
    for i in range({rows}):
        n = i % 1024
        items.append(Particle(n, n + 1, n + 2, n + 3, n + 4, n + 5, n + 6, n + 7))?
    mut total = 0
    for repeat in range({repeats}):
        total += native_{kernel}(&items)
    print(total)?
    Ok(())
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--rows", type=int, default=8192)
    parser.add_argument("--repeats", type=int, default=200)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--cpu", type=int, default=8)
    args = parser.parse_args()
    assert args.rows > 0 and args.repeats > 0 and args.samples > 0
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "columns.plenty").write_text(generate((HERE / "particle.plenty").read_text()))
    env = {**os.environ, "PLENTY_CACHE": "off", "PLENTY_LINKER_THREADS": "1"}
    report = {"rows": args.rows, "repeats": args.repeats, "samples": args.samples,
              "cpu": args.cpu, "events": "instructions:u,cycles:u",
              "scope": "entire process, including construction and teardown", "compilers": {}, "results": []}
    binaries = {}
    for label in ("before", "after"):
        compiler = getattr(args, label).resolve()
        report["compilers"][label] = {"path": str(compiler), "sha256": hashlib.sha256(compiler.read_bytes()).hexdigest()}
        for kernel in ("control", "stream", "whole"):
            path = args.out / f"{label}_{kernel}.plenty"
            binary = path.with_suffix("")
            path.write_text(source(args.rows, 0 if kernel == "control" else args.repeats,
                                   "stream" if kernel == "control" else kernel))
            subprocess.run([str(compiler), "--release", "--compile", str(path), "-o", str(binary)], env=env, check=True)
            binaries[label, kernel] = binary.resolve()
    # All compilation precedes counting. Alternate versions to expose drift.
    samples = {(label, kernel): [] for label, kernel in binaries}
    for sample in range(args.samples):
        for kernel in ("control", "stream", "whole"):
            for label in (("before", "after") if sample % 2 == 0 else ("after", "before")):
                binary = binaries[label, kernel]
                run = subprocess.run(["perf", "stat", "-x", ",", "-e", report["events"],
                                      "--", "taskset", "-c", str(args.cpu), str(binary)],
                                     env=env, text=True, capture_output=True, check=True)
                checksum = 0 if kernel == "control" else expected(args.rows, kernel) * args.repeats
                assert run.stdout.strip() == str(checksum), run.stdout
                counters = {}
                for line in run.stderr.splitlines():
                    fields = line.split(",")
                    if len(fields) >= 3 and fields[2] in ("instructions:u", "cycles:u"):
                        counters[fields[2]] = int(fields[0])
                assert len(counters) == 2, run.stderr
                samples[label, kernel].append(counters)
                (args.out / f"{label}_{kernel}_{sample}.perf").write_text(run.stderr)
    for (label, kernel), values in samples.items():
        binary = binaries[label, kernel]
        record = {"version": label, "kernel": kernel, "samples": values,
                  "source_sha256": hashlib.sha256(binary.with_suffix(".plenty").read_bytes()).hexdigest(),
                  "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                  "median_instructions": statistics.median(v["instructions:u"] for v in values)}
        report["results"].append(record)
        print(label, kernel, record["median_instructions"], "instructions")
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
