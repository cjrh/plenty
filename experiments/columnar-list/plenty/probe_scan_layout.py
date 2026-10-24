#!/usr/bin/env python3
"""Bounded x86-64/Linux placement probe: relink one unchanged Plenty object.

Requires GNU ld (via cc -fuse-ld=bfd), nm, and user-mode perf counters. This
changes the whole text-section address, not compiler policy or loop semantics.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import random
import statistics
import subprocess

from bench import expected
from count_access import source
from generate import generate

HERE = Path(__file__).resolve().parent


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("compiler", type=Path)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--rows", type=int, default=8192)
    p.add_argument("--repeats", type=int, default=20000)
    p.add_argument("--samples", type=int, default=3)
    p.add_argument("--cpu", type=int, default=8)
    args = p.parse_args()
    assert args.rows > 0 and args.repeats > 0 and args.samples > 0
    compiler, out = args.compiler.resolve(), args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    program = source(args.rows, args.repeats, "one")
    start = "    mut total = 0\n    for repeat"
    stop = "    print(total)?"
    assert program.count(start) == program.count(stop) == 1
    program = program.replace(start, "    begin = monotonic_now()\n" + start)
    program = program.replace(stop, "    end = monotonic_now()\n    print(total, end.saturating_duration_since(&begin).as_nanoseconds())?")
    path = out / "scan.plenty"
    path.write_text(program)
    (out / "columns.plenty").write_text(generate((HERE / "particle.plenty").read_text()))
    env = {**os.environ, "PLENTY_CACHE": "off", "LC_ALL": "C"}
    obj = out / "scan.o"
    # Older compiler builds do not accept --release with --emit-object.
    # Capture the exact release object and archive at the linker boundary.
    wrapper = out / "capture-link.py"
    wrapper.write_text('''#!/usr/bin/env python3
import json, shutil, subprocess, sys
from pathlib import Path
out = Path(__file__).parent
shutil.copyfile(sys.argv[1], out / "scan.o")
shutil.copyfile(sys.argv[2], out / "runtime.a")
assert sys.argv[-2] == "-o"
(out / "link-args.json").write_text(json.dumps(sys.argv[3:-2]))
subprocess.run(["cc", "-fuse-ld=bfd", *sys.argv[1:]], check=True)
''')
    wrapper.chmod(0o755)
    subprocess.run([str(compiler), "--release", "--compile", str(path), "-o", str(out / "seed"),
                    "--linker", str(wrapper)], env=env, check=True)
    link_args = json.loads((out / "link-args.json").read_text())
    variants = {}
    for offset in range(0, 64, 8):
        binary = out / f"scan_{offset}"
        address = 0x10000000 + offset
        subprocess.run(["cc", "-fuse-ld=bfd", str(obj), str(out / "runtime.a"),
                        *link_args, f"-Wl,--section-start=.text=0x{address:x}", "-o", str(binary)], check=True, capture_output=True)
        symbols = subprocess.run(["nm", "-S", str(binary)], text=True, capture_output=True, check=True).stdout
        entry = next(l.split() for l in symbols.splitlines() if l.endswith(" __plenty_fn_native_one"))
        variants[offset] = {"binary": str(binary), "function_address": int(entry[0], 16),
                            "function_bytes": int(entry[1], 16), "samples": []}
    order = list(variants)
    rng = random.Random(437)
    for sample in range(args.samples + 1):
        rng.shuffle(order)
        for offset in order:
            v = variants[offset]
            run = subprocess.run(["perf", "stat", "-x", ",", "-e", "instructions:u,cycles:u",
                                  "--", "taskset", "-c", str(args.cpu), v["binary"]],
                                 env=env, text=True, capture_output=True, check=True)
            checksum, ns = map(int, run.stdout.split())
            assert checksum == expected(args.rows, "one") * args.repeats
            counts = {}
            for line in run.stderr.splitlines():
                fields = line.split(",")
                if len(fields) >= 3 and fields[2] in ("instructions:u", "cycles:u"):
                    counts[fields[2]] = int(fields[0])
            assert len(counts) == 2, run.stderr
            if sample:
                v["samples"].append({"ns_per_row": ns / (args.rows * args.repeats), **counts})
    for offset, v in variants.items():
        v["median_ns_per_row"] = statistics.median(s["ns_per_row"] for s in v["samples"])
        v["median_cycles_per_row"] = statistics.median(s["cycles:u"] for s in v["samples"]) / (args.rows * args.repeats)
        print(offset, hex(v["function_address"]), round(v["median_ns_per_row"], 3), "ns/row",
              round(v["median_cycles_per_row"], 3), "cycles/row", flush=True)
    report = {"compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
              "object_sha256": hashlib.sha256(obj.read_bytes()).hexdigest(),
              "rows": args.rows, "repeats": args.repeats, "samples": args.samples, "cpu": args.cpu,
              "method": "One object, GNU ld text starts 0x10000000+[0,8,...,56]; shuffled order seed 437; warmup excluded; perf counters include construction/startup, internal timing covers repeated folds.",
              "variants": variants}
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
