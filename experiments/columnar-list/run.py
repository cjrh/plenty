#!/usr/bin/env python3
"""Build isolated kernels, retain samples, and summarize medians and ranges."""
import argparse
import csv
import io
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess

ROOT = Path(__file__).resolve().parent


def run(args, **kwargs):
    return subprocess.run(args, check=True, text=True, capture_output=True, **kwargs).stdout


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--cpu", type=int, default=None, help="optional Linux CPU affinity")
    args = parser.parse_args()
    output = ROOT / "out"
    output.mkdir(exist_ok=True)
    metadata = {
        "platform": platform.platform(),
        "rustc": run(["rustc", "-Vv"]),
        "lscpu": run(["lscpu"]),
        "cpu": args.cpu,
        "load_average_before": os.getloadavg(),
        "modes": {},
    }
    for mode, flags in {
        "native": "-C target-cpu=native",
        "scalar": "-C target-cpu=native -C no-vectorize-loops -C no-vectorize-slp",
    }.items():
        env = os.environ.copy()
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        env["RUSTFLAGS"] = flags
        target = ROOT / "target" / mode
        env["CARGO_TARGET_DIR"] = str(target)
        run(["cargo", "rustc", "--locked", "--release", "--", "--emit=asm,link"], cwd=ROOT, env=env)
        command = [str(target / "release" / "columnar-list-spike")]
        if args.cpu is not None:
            command = ["taskset", "-c", str(args.cpu)] + command
        raw = run(command, cwd=ROOT)
        (output / f"{mode}.csv").write_text(raw)
        groups = {}
        for row in csv.DictReader(io.StringIO(raw)):
            key = (int(row["rows"]), row["kernel"], row["layout"])
            groups.setdefault(key, []).append(float(row["ns_per_row"]))
        lines = ["rows,kernel,layout,median_ns_per_row,min_ns_per_row,max_ns_per_row"]
        for key, times in groups.items():
            lines.append(",".join(map(str, (*key, statistics.median(times), min(times), max(times)))))
        (output / f"{mode}-summary.csv").write_text("\n".join(lines) + "\n")
        metadata["modes"][mode] = flags
        print(f"{mode}: {len(groups)} cases, seven samples each; results in {output}")
    metadata["load_average_after"] = os.getloadavg()
    (output / "machine.json").write_text(json.dumps(metadata, indent=2) + "\n")


if __name__ == "__main__":
    main()
