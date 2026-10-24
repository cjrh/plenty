#!/usr/bin/env python3
"""Bounded x86-64/Linux placement probe: relink one unchanged Plenty object.

Requires GNU binutils (ld, nm, objcopy, objdump, size) and user-mode perf counters.
Relinks unchanged function bytes at different addresses and optional function
alignments. This is an experiment, not a compiler policy or loop-alignment fix.
"""
import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import random
import re
import shutil
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
    p.add_argument("--alignments", type=int, nargs="+", default=[1],
                   choices=[1, 16, 32, 64], help="function-section alignments; 1 preserves the original")
    p.add_argument("--kernel", choices=["one", "two", "scalar"], default="one")
    args = p.parse_args()
    assert args.rows > 0 and args.repeats > 0 and args.samples > 0
    compiler, out = args.compiler.resolve(), args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    program = source(args.rows, args.repeats, args.kernel)
    symbol = f"__plenty_fn_native_{args.kernel}"
    checksum_expected = expected(args.rows, args.kernel) * args.repeats
    if args.kernel == "scalar":
        # An unrelated checked-arithmetic loop, with no collection operations.
        program = f'''from std.time import monotonic_now

def scalar(stop: i64) -> i64:
    mut total = 0
    for i in range(stop):
        total += i
    total

def main() -> Result[(), IoError]:
    mut total = 0
    for repeat in range({args.repeats}):
        total += scalar({args.rows})
    print(total)?
    Ok(())
'''
        symbol = "__plenty_fn_scalar"
        checksum_expected = args.rows * (args.rows - 1) // 2 * args.repeats
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
    section_headers = subprocess.check_output(["objdump", "-h", str(obj)], text=True)
    section_alignments = [1 << int(n) for n in re.findall(r"\.text.subsection.*2\*\*(\d+)", section_headers)]
    assert section_alignments, "expected Cranelift function sections"
    def function_contents(path):
        dump = subprocess.check_output(["objdump", "-s", "-j", ".text.subsection", str(path)], text=True)
        # Ignore the filename header; retain all function section contents.
        return dump[dump.index("Contents of section"):]
    original_contents = function_contents(obj)
    variants = {}
    for alignment in args.alignments:
        aligned = out / f"scan_align_{alignment}.o"
        # Cranelift ObjectModule emits one identically named section per function.
        # objcopy changes only section alignment; code and relocations are retained.
        if alignment == 1:
            shutil.copyfile(obj, aligned)  # Preserve existing constant-pool alignment too.
        else:
            assert max(section_alignments) <= alignment, "probe must never lower required alignment"
            subprocess.run(["objcopy", "--set-section-alignment", f".text.subsection={alignment}",
                            str(obj), str(aligned)], check=True)
        assert function_contents(aligned) == original_contents, "function bytes changed"
        for offset in range(0, 64, 8):
            binary = out / f"scan_{alignment}_{offset}"
            address = 0x10000000 + offset
            subprocess.run(["cc", "-fuse-ld=bfd", str(aligned), str(out / "runtime.a"),
                            *link_args, f"-Wl,--section-start=.text=0x{address:x}", "-o", str(binary)], check=True, capture_output=True)
            symbols = subprocess.run(["nm", "-S", str(binary)], text=True, capture_output=True, check=True).stdout
            entry = next(l.split() for l in symbols.splitlines() if l.endswith(" " + symbol))
            function_address = int(entry[0], 16)
            assert function_address % alignment == 0
            sections = subprocess.check_output(["size", "-A", str(binary)], text=True)
            text_bytes = int(next(l.split()[1] for l in sections.splitlines() if l.startswith(".text ")))
            variants[f"{alignment}_{offset}"] = {
                "binary": str(binary), "alignment": alignment, "text_offset": offset,
                "function_address": function_address, "function_bytes": int(entry[1], 16),
                "text_bytes": text_bytes, "binary_bytes": binary.stat().st_size, "samples": []}
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
            assert checksum == checksum_expected
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
              "source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
              "platform": platform.platform(), "kernel": args.kernel, "alignments": args.alignments,
              "object_sha256": hashlib.sha256(obj.read_bytes()).hexdigest(),
              "rows": args.rows, "repeats": args.repeats, "samples": args.samples, "cpu": args.cpu,
              "method": "One object; objcopy function-section alignment variants; GNU ld text starts 0x10000000+[0,8,...,56]; shuffled order seed 437; warmup excluded; perf counters include construction/startup, internal timing covers repeated kernels.",
              "variants": variants}
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
