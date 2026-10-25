#!/usr/bin/env python3
"""Bounded ordered clist removal cost by row width and position.

Build Plenty without runtime-checks; generated programs use --release. Each
sample constructs fresh rows before the clock, then removes and appends to
maintain a fixed length. Reported times include return-row gathering and append.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess


def program(fields, rows, repeats, samples):
    schema = "\n".join(f"    f{i}: i64" for i in range(fields))
    values = ", ".join("n" for _ in range(fields))
    source = f"""from std.time import monotonic_now
class Row:
{schema}
def row(n: i64) -> Row:
    Row({values})
def main() -> Result[(), AllocError | AppendError[Row] | IoError]:
"""
    for name, index in [("front", "0"), ("middle", str(rows // 2)), ("last", "-1")]:
        source += f"""    for sample in range({samples + 1}):
        mut items = clist[Row].with_capacity({rows})?
        for n in range({rows}):
            items.append(row(n))?
        start = monotonic_now()
        mut total = 0
        for n in range({repeats}):
            removed = items.remove({index}).unwrap()
            total += removed.f0
            items.append(row(-1))?
        stop = monotonic_now()
        print("{name}", sample, stop.saturating_duration_since(&start).as_nanoseconds(), total)?
"""
    return source + "    Ok(())\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cpu", type=int)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "PLENTY_CACHE": "off", "PLENTY_LINKER_THREADS": "1"}
    repeats, samples = 256, 7
    results = []
    for rows in [8192, 65536]:
        for fields in [1, 4, 16]:
            path = args.out / f"remove_{rows}_{fields}.plenty"
            binary = path.with_suffix("")
            path.write_text(program(fields, rows, repeats, samples))
            subprocess.run([str(args.compiler.resolve()), "--release", "--compile", str(path), "-o", str(binary)], env=env, check=True)
            command = [str(binary.resolve())]
            if args.cpu is not None:
                command = ["taskset", "-c", str(args.cpu), *command]
            output = subprocess.check_output(command, env=env, text=True)
            (args.out / f"raw_{rows}_{fields}.txt").write_text(output)
            timings = {}
            expected = {"front": repeats * (repeats - 1) // 2,
                        "middle": repeats * (rows // 2) + repeats * (repeats - 1) // 2,
                        "last": rows - repeats}
            for line in output.splitlines():
                name, sample, nanos, checksum = line.split()
                assert int(checksum) == expected[name], line
                if int(sample):
                    timings.setdefault(name, []).append(int(nanos) / repeats)
            for name, values in timings.items():
                result = dict(rows=rows, fields=fields, row_bytes=fields * 8, position=name,
                              median_ns=statistics.median(values), samples_ns=values)
                results.append(result)
                print(rows, fields * 8, name, round(result["median_ns"], 1), flush=True)
    report = dict(cpu=args.cpu, repeats=repeats, samples=samples, results=results,
                  platform=platform.platform(),
                  compiler_sha256=hashlib.sha256(args.compiler.read_bytes()).hexdigest())
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
