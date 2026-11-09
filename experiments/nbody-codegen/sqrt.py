#!/usr/bin/env python3
"""Measure the public SIMD magnitude path, retaining scalar benchmark source."""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
REPLACEMENT = """        step = simd.splat_f64x2(dt)
        for pair in range(pairs // 2):
            p = pair * 2
            a = simd.load_f64x2(&dx, p).unwrap()
            b = simd.load_f64x2(&dy, p).unwrap()
            c = simd.load_f64x2(&dz, p).unwrap()
            d2 = a.mul(a).add(b.mul(b)).add(c.mul(c))
            result = step.div(d2.mul(d2.sqrt()))
            check(result.store(&mut mag, p), "output capacity").unwrap()
        if pairs % 2 == 1:
            p = pairs - 1
            d2 = dx[p] * dx[p] + dy[p] * dy[p] + dz[p] * dz[p]
            mag[p] = dt / (d2 * d2.sqrt())
"""

def main():
    work = ROOT / "target/simd-sqrt"
    work.mkdir(parents=True, exist_ok=True)
    source = (ROOT / "benchmarks/game/n-body/plenty/nbody.plenty").read_text()
    vector = source.replace("import std.math", "import std.math\nimport std.simd as simd\nfrom std.test import check")
    start = vector.index("        for p in range(pairs):")
    end = vector.index("        k = 0", start)
    vector = vector[:start] + REPLACEMENT + vector[end:]
    env = dict(os.environ, PLENTY_CACHE="off")
    results = {}
    for name, text in [("scalar", source), ("vector", vector)]:
        path = work / (name + ".plenty")
        path.write_text(text)
        exe = work / name
        subprocess.run([ROOT / "target/release/plenty", "--compile", path, "-o", exe], env=env, check=True)
        results[name] = []
        for repeat in range(3):
            counter = work / f"{name}-{repeat}.perf"
            output = subprocess.check_output(["perf", "stat", "-x", ";", "-e", "instructions:u", "-o", counter, "taskset", "-c", "0", exe, "100000"], text=True)
            if name == "scalar":
                expected = output
            elif output != expected:
                raise RuntimeError("n-body output mismatch")
            results[name].append(next(int(line.split(";")[0]) for line in counter.read_text().splitlines() if ";instructions:u;" in line))
    (work / "counts.json").write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))

if __name__ == "__main__":
    main()
