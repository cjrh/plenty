#!/usr/bin/env python3
"""Issue #655: build and run the clist bitpacking measurements.

Usage: run.py PLENTY_COMPILER --out DIR [--cpus 0,2,4,6,8,10,12,14] [--quick]

Builds bench.c (SSE2 baseline, gcc -O2 without auto-vectorization), runs it
for each size, counts the per-row instructions of Plenty's current clist
filter/count paths, and writes DIR/results.json plus Markdown tables on
stdout. --quick runs only the cache-resident size.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROWS = 1 << 20


def host_load():
    load = Path("/proc/loadavg").read_text().split()[:3]
    busy = subprocess.run(["pgrep", "-c", "-x", "rustc"], capture_output=True, text=True).stdout.strip()
    return {"loadavg": load, "rustc_processes": int(busy or 0)}


def perf_instructions(cmd):
    out = subprocess.run(
        ["perf", "stat", "-x,", "-e", "instructions:u"] + cmd,
        capture_output=True, text=True, check=True,
    )
    for line in out.stderr.splitlines():
        if "instructions" in line:
            return int(line.split(",")[0])
    raise RuntimeError(out.stderr)


def plenty_counts(compiler, out, cpus):
    src = (HERE / "scan.plenty").read_text()
    results = {}
    env = dict(os.environ, PLENTY_CACHE="off")
    for kind, name in [(0, "i32"), (1, "u16"), (2, "u8")]:
        counts = {}
        for reps in (1, 101):
            text = re.sub(r"^const REPS: i64 = \d+$", f"const REPS: i64 = {reps}", src, flags=re.M)
            text = re.sub(r"^const KIND: i64 = \d+$", f"const KIND: i64 = {kind}", text, flags=re.M)
            path = out / f"scan_{kind}_{reps}.plenty"
            exe = out / f"scan_{kind}_{reps}"
            path.write_text(text)
            subprocess.run([compiler, "--compile", str(path), "-o", str(exe)], check=True, env=env)
            counts[reps] = perf_instructions(["taskset", "-c", cpus, str(exe)])
        results[name] = (counts[101] - counts[1]) / (100 * ROWS)
    return results


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("compiler")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--cpus", default="0,2,4,6,8,10,12,14")
    ap.add_argument("--quick", action="store_true")
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    exe = args.out / "bench"
    subprocess.run(
        ["gcc", "-O2", "-fno-tree-vectorize", "-Wall", "-Wextra", "-Werror",
         "-o", str(exe), str(HERE / "bench.c")],
        check=True,
    )
    report = {"before": host_load(), "plenty_instr_per_row": None, "rows": []}
    report["plenty_instr_per_row"] = plenty_counts(args.compiler, args.out, args.cpus)
    for size in ([0] if args.quick else [0, 1, 2]):
        run = subprocess.run(
            ["taskset", "-c", args.cpus, str(exe), str(size)],
            capture_output=True, text=True, check=True,
        )
        report["rows"] += [json.loads(line) for line in run.stdout.splitlines()]
    report["after"] = host_load()
    (args.out / "results.json").write_text(json.dumps(report, indent=1) + "\n")
    print_tables(report)


def print_tables(report):
    print("Host load before/after:", report["before"], report["after"])
    print("Plenty instructions/row (filter > c, count):", report["plenty_instr_per_row"])
    scans = [r for r in report["rows"] if r["group"] == "scan"]
    kernels = []
    for r in scans:
        if r["kernel"] not in kernels:
            kernels.append(r["kernel"])
    for sel in ("50%", "1%"):
        print(f"\n### Scan ns/row (instructions/row), {sel} of rows match\n")
        print("| Data | Rows | " + " | ".join(kernels) + " |")
        print("|---|---:|" + "---:|" * len(kernels))
        keys = []
        for r in scans:
            k = (r["dist"], r["rows"])
            if k not in keys:
                keys.append(k)
        for dist, rows in keys:
            cells = []
            for kern in kernels:
                m = [r for r in scans if (r["dist"], r["rows"], r["selectivity"], r["kernel"]) == (dist, rows, sel, kern)]
                cells.append(f"{m[0]['ns_per_row']:.3f} ({m[0]['instr_per_row']:.2f})" if m else "")
            print(f"| {dist} | {rows} | " + " | ".join(cells) + " |")
    print("\n### Bytes/row including block metadata\n")
    seen = set()
    for r in scans:
        k = (r["dist"], r["kernel"])
        if k in seen or r["rows"] != 1 << 20 or r["selectivity"] != "50%":
            continue
        seen.add(k)
        print(f"| {r['dist']} | {r['kernel']} | {r['bytes_per_row']:.3f} |")
    print("\n### Operations (1,048,576 rows)\n")
    for r in report["rows"]:
        if r["group"] == "op":
            print(f"| {r['dist']} | {r['op']} | {r['ns']:.3f} ns/{r['unit']} | {r['instr']:.1f} instr/{r['unit']} |")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--tables":
        print_tables(json.loads(Path(sys.argv[2]).read_text()))
    else:
        main()
