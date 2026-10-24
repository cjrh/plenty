#!/usr/bin/env python3
"""Compile real Plenty pipelines, verify guard pages, and compare code placements.

Linux x86-64, GNU binutils and perf are required. The C fixture uses internal
read-only clist ABI solely to put the actual generated scan against a guard page.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess
import tomllib

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--cpu", type=int, default=8)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--verify-only", action="store_true")
    args = parser.parse_args()
    out, compiler = args.out.resolve(), args.compiler.resolve()
    out.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "PLENTY_CACHE": "off", "LC_ALL": "C"}
    wrapper = out / "capture.py"
    wrapper.write_text('''#!/usr/bin/env python3
import json, shutil, subprocess, sys
from pathlib import Path
out = Path(__file__).parent
shutil.copyfile(sys.argv[1], out / "scan.o")
shutil.copyfile(sys.argv[2], out / "runtime.a")
assert sys.argv[-2] == "-o"
(out / "link.json").write_text(json.dumps(sys.argv[3:-2]))
subprocess.run(["cc", "-fuse-ld=bfd", *sys.argv[1:]], check=True)
''')
    wrapper.chmod(0o755)
    subprocess.run([str(compiler), "--release", "--compile", str(HERE / "scan.plenty"),
                    "--linker", str(wrapper), "-o", str(out / "seed")], env=env, check=True)
    subprocess.run([str(compiler), "--release", "--dump", "clif,asm", str(HERE / "scan.plenty"),
                    "-o", str(out / "dump")], env=env, check=True)
    clif = (out / "dump/clif.txt").read_text()
    vector = clif.split("==== simd_count ")[1].split("==== ")[0]
    scalar = clif.split("==== scalar_count ")[1].split("==== ")[0]
    assert "load.i32x4" in vector and "load.i32x4" not in scalar
    subprocess.run(["objcopy", "--redefine-sym", "main=unused_plenty_main",
                    "--rename-section", ".text.subsection=.plenty_scan_text",
                    "--globalize-symbol", "__plenty_fn_simd_count",
                    "--globalize-symbol", "__plenty_fn_scalar_count",
                    str(out / "scan.o"), str(out / "harness-scan.o")], check=True)
    subprocess.run(["objcopy", "--redefine-sym", "main=unused_runtime_main",
                    str(out / "runtime.a"), str(out / "harness-runtime.a")], check=True)
    subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-c",
                    str(HERE / "harness.c"), "-o", str(out / "harness.o")], check=True)
    link = json.loads((out / "link.json").read_text())
    variants = {}
    for offset in ([0] if args.verify_only else [0, 16, 32, 48]):
        binary = out / f"scan-{offset}"
        subprocess.run(["cc", "-fuse-ld=bfd", str(out / "harness.o"), str(out / "harness-scan.o"),
                        str(out / "harness-runtime.a"), *link,
                        f"-Wl,--section-start=.plenty_scan_text=0x{0x10000000 + offset:x}", "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)
        symbols = subprocess.check_output(["nm", "-S", str(binary)], text=True)
        for mode in [0, 1]:
            kind = "scalar" if mode else "simd"
            symbol = next(line.split() for line in symbols.splitlines()
                          if line.endswith(f" __plenty_fn_{kind}_count"))
            if kind == "simd":
                assert int(symbol[0], 16) % 64 == offset
            for rows in [7, 8192, 262144]:
                repeats = max(1, 50_000_000 // rows)
                variants[f"{kind}-{rows}-{offset}"] = dict(kind=kind, rows=rows, repeats=repeats,
                    offset=offset, address=int(symbol[0], 16), bytes=int(symbol[1], 16), samples=[])
    if args.verify_only:
        return
    order = list(variants)
    rng = random.Random(23)
    for sample in range(args.samples + 1):
        rng.shuffle(order)
        for key in order:
            v = variants[key]
            run = subprocess.run(["perf", "stat", "-x", ",", "-e", "instructions:u,cycles:u", "--",
                "taskset", "-c", str(args.cpu), str(out / f"scan-{v['offset']}"),
                str(v["rows"]), str(v["repeats"]), str(int(v["kind"] == "scalar"))],
                env=env, text=True, capture_output=True, check=True)
            checksum, ns = map(int, run.stdout.split())
            counts = {}
            for line in run.stderr.splitlines():
                fields = line.split(",")
                if len(fields) >= 3 and fields[2] in ("instructions:u", "cycles:u"):
                    counts[fields[2]] = int(fields[0])
            assert len(counts) == 2, run.stderr
            if sample:
                v["samples"].append(dict(checksum=checksum, ns=ns, **counts))
    for key, v in variants.items():
        work = v["rows"] * v["repeats"]
        for metric in ["ns", "instructions:u", "cycles:u"]:
            v[f"median_{metric}_per_row"] = statistics.median(s[metric] for s in v["samples"]) / work
        print(key, round(v["median_ns_per_row"], 3), "ns/row", flush=True)
    lock = tomllib.loads((HERE.parents[2] / "Cargo.lock").read_text())
    cranelift = next(p["version"] for p in lock["package"] if p["name"] == "cranelift-codegen")
    report = dict(platform=platform.platform(), cpu=args.cpu, samples=args.samples,
        cranelift_version=cranelift,
        compiler_sha256=hashlib.sha256(compiler.read_bytes()).hexdigest(),
        object_sha256=hashlib.sha256((out / "scan.o").read_bytes()).hexdigest(),
        method="One object; Plenty function sections moved together to four verified text starts 16 bytes apart; randomized order seed 23; one warmup excluded. Internal timings cover repeated calls; perf includes process setup and input construction. Captured threshold forces the scalar reference path; both thresholds equal 5.",
        variants=variants)
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
