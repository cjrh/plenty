#!/usr/bin/env python3
"""Compare already-built before/after programs with the same stdin.

Usage: measure.py BEFORE AFTER INPUT OUTPUT_JSON [REPETITIONS]
Instruction counts include all children; timings alternate on physical core 0.
"""
import json
from pathlib import Path
import subprocess
import sys
import time

before, after, source, output, *rest = sys.argv[1:]
repeats = int(rest[0]) if rest else 7
result = {name: [] for name in ("before", "after")}
for repeat in range(repeats):
    order = [("before", before), ("after", after)]
    if repeat % 2:
        order.reverse()
    outputs = []
    for name, executable in order:
        counter = Path(output).with_suffix(f".{name}-{repeat}.perf")
        with open(source, "rb") as inp:
            start = time.perf_counter()
            observed = subprocess.check_output([
                "perf", "stat", "-x", ";", "-e", "instructions:u", "-o", counter,
                "taskset", "-c", "0", str(Path(executable).resolve())], stdin=inp)
            elapsed = time.perf_counter() - start
        count = next(int(line.split(";")[0]) for line in counter.read_text().splitlines()
                     if ";instructions:u;" in line)
        result[name].append({"instructions": count, "seconds_including_perf": elapsed})
        outputs.append(observed)
    if outputs[0] != outputs[1]:
        raise RuntimeError("output mismatch")
Path(output).write_text(json.dumps(result, indent=2) + "\n")
print(json.dumps(result, indent=2))
