#!/usr/bin/env python3
"""Count the public SIMD list-transfer path: the magnitude kernel and n-body
variants whose magnitude loop uses it. The benchmark source stays scalar.

Usage: measure.py [COMPILER]   (default target/release/plenty)
"""
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
STEPS = "500000"
TAIL = """        if pairs % 2 == 1:
            p = pairs - 1
            d2 = dx[p] * dx[p] + dy[p] * dy[p] + dz[p] * dz[p]
            mag[p] = dt / (d2 * d2.sqrt())
"""
UNWRAP = """        step = simd.splat_f64x2(dt)
        for pair in range(pairs // 2):
            p = pair * 2
            a = simd.load_f64x2(&dx, p).unwrap()
            b = simd.load_f64x2(&dy, p).unwrap()
            c = simd.load_f64x2(&dz, p).unwrap()
            d2 = a.mul(a).add(b.mul(b)).add(c.mul(c))
            result = step.div(d2.mul(d2.sqrt()))
            check(result.store(&mut mag, p), "output capacity").unwrap()
""" + TAIL
MATCH = """        step = simd.splat_f64x2(dt)
        for pair in range(pairs // 2):
            p = pair * 2
            match simd.load_f64x2(&dx, p):
                case Some(a):
                    match simd.load_f64x2(&dy, p):
                        case Some(b):
                            match simd.load_f64x2(&dz, p):
                                case Some(c):
                                    d2 = a.mul(a).add(b.mul(b)).add(c.mul(c))
                                    stored = step.div(d2.mul(d2.sqrt())).store(&mut mag, p)
                                case Nothing:
                                    pass
                        case Nothing:
                            pass
                case Nothing:
                    pass
""" + TAIL


def count(command):
    counter = HERE / "counter.tmp"
    output = subprocess.check_output(
        ["perf", "stat", "-x", ";", "-e", "instructions:u", "-o", counter, "taskset", "-c", "2", *command],
        text=True,
    )
    value = next(int(line.split(";")[0]) for line in counter.read_text().splitlines() if ";instructions:u;" in line)
    counter.unlink()
    return value, output


def main():
    compiler = Path(sys.argv[1] if len(sys.argv) > 1 else ROOT / "target/release/plenty").resolve()
    work = ROOT / "target/simd-transfers" / compiler.name
    work.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PLENTY_CACHE="off")

    def build(name, path):
        exe = work / name
        subprocess.run([compiler, "--compile", path, "-o", exe], env=env, check=True)
        return exe

    results = {}
    kernel = build("magnitude", HERE / "magnitude.plenty")
    expected = None
    for mode in ["scalar", "simd"]:
        low, output = count([kernel, mode, "1000", "100000"])
        high, _ = count([kernel, mode, "1000", "200000"])
        if expected not in (None, output):
            raise RuntimeError("kernel output mismatch")
        expected = output
        # 500 two-pair groups per pass over 1,000 elements.
        results[f"kernel {mode}, instructions per two pairs"] = (high - low) / 100000 / 500

    source = (ROOT / "benchmarks/game/n-body/plenty/nbody.plenty").read_text()
    header = source.replace("import std.math", "import std.math\nimport std.simd as simd\nfrom std.test import check")
    start = header.index("        for p in range(pairs):")
    end = header.index("        k = 0", start)
    expected = None
    for name, text in [("scalar", source), ("unwrap", header[:start] + UNWRAP + header[end:]),
                       ("match", header[:start] + MATCH + header[end:])]:
        path = work / f"nbody-{name}.plenty"
        path.write_text(text)
        value, output = count([build(f"nbody-{name}", path), STEPS])
        if expected not in (None, output):
            raise RuntimeError("n-body output mismatch")
        expected = output
        results[f"n-body {name}, {STEPS} steps"] = value
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
