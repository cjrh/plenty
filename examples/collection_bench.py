"""Reproducible collection timings; see collection-bench.md for interpretation."""

import argparse
import hashlib
import json
import pathlib
import platform
import statistics
import subprocess
import time


def workloads():
    for n in (2_000_000, 8_000_000):
        setup = "mut xs = [i for i in range(1024)].unwrap()"
        for name, operation, expected in (
            ("read", "total = total + xs[i % 1024]", sum(range(1024)) * (n // 1024) + sum(range(n % 1024))),
            ("write", "xs[i % 1024] = i", n - 1),
            ("len", "total = total + len(xs)", n * 1024),
        ):
            result = "xs[(i - 1) % 1024]" if name == "write" else "total"
            yield name, n, f"{setup}\nmut total = 0\nmut i = 0\nwhile i < {n}:\n    {operation}\n    i = i + 1\nprint({result}).unwrap()", expected
    for n in (200_000, 800_000):
        yield "nested", n, f"xs = [[i for i in range(32)].unwrap() for j in range(32)].unwrap()\nmut total = 0\nmut i = 0\nwhile i < {n}:\n    total = total + xs[i % 32][i % 32]\n    i = i + 1\nprint(total).unwrap()", 496 * (n // 32)
        yield "hash", n, f"xs = {{i: i for i in range(1024)}}.unwrap()\nmut total = 0\nmut i = 0\nwhile i < {n}:\n    if i % 2048 in xs:\n        total = total + 1\n    i = i + 1\nprint(total).unwrap()", 1024 * (n // 2048) + min(1024, n % 2048)
    for n in (2000, 8000):
        yield "front", n, f"mut xs = [i for i in range({n})].unwrap()\nmut total = 0\nwhile len(xs) > 0:\n    total = total + xs.pop(0).unwrap()\nprint(total).unwrap()", n * (n - 1) // 2


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--compiler", required=True, type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    compiler = args.compiler.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    metadata = {"compiler": str(compiler), "sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(), "host": platform.platform(), "machine": platform.machine(), "options": "--compile FILE -o OUT", "runs": args.runs}
    print(json.dumps(metadata))
    results = []
    for name, n, body, expected in workloads():
        source = args.output / f"{name}-{n}.plenty"
        executable = source.with_suffix("")
        source.write_text("def main() -> ():\n" + "\n".join("    " + line for line in body.splitlines()) + "\n")
        start = time.perf_counter()
        subprocess.run([compiler, "--compile", source, "-o", executable], check=True, stdout=subprocess.DEVNULL)
        compile_seconds = time.perf_counter() - start
        times = []
        for _ in range(args.runs):
            start = time.perf_counter()
            result = subprocess.run([executable.resolve()], check=True, capture_output=True, text=True)
            times.append(time.perf_counter() - start)
            assert result.stdout.strip() == str(expected), (source, result.stdout, expected)
        result = {"workload": name, "size": n, "checksum": expected, "compile_seconds": compile_seconds, "run_seconds": times, "median_seconds": statistics.median(times)}
        results.append(result)
        print(json.dumps(result), flush=True)
    (args.output / "results.json").write_text(json.dumps({"metadata": metadata, "results": results}, indent=2) + "\n")


if __name__ == "__main__":
    main()
