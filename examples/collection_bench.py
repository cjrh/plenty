"""Reproducible collection timings; see collection-bench.md for interpretation."""

import argparse
import hashlib
import json
import os
import pathlib
import platform
import statistics
import subprocess
import time


def workloads():
    for n in (2_000_000, 8_000_000):
        setup = "mut xs = [i for i in range(1024)]?"
        for name, operation, expected in (
            ("read", "total = total + xs[i % 1024]", sum(range(1024)) * (n // 1024) + sum(range(n % 1024))),
            ("write", "xs[i % 1024] = i", n - 1),
            ("len", "total = total + len(xs)", n * 1024),
        ):
            result = "xs[(i - 1) % 1024]" if name == "write" else "total"
            yield name, n, f"{setup}\nmut total = 0\nmut i = 0\nwhile i < {n}:\n    {operation}\n    i = i + 1\nprint({result})?", expected
    for n in (200_000, 800_000):
        yield "nested", n, f"xs = [[i for i in range(32)]? for j in range(32)]?\nmut total = 0\nmut i = 0\nwhile i < {n}:\n    total = total + xs[i % 32][i % 32]\n    i = i + 1\nprint(total)?", 496 * (n // 32)
        yield "hash", n, f"xs = {{i: i for i in range(1024)}}?\nmut total = 0\nmut i = 0\nwhile i < {n}:\n    if i % 2048 in xs:\n        total = total + 1\n    i = i + 1\nprint(total)?", 1024 * (n // 2048) + min(1024, n % 2048)
    for n in (2000, 8000):
        yield "front", n, f"mut xs = [i for i in range({n})]?\nmut total = 0\nwhile len(xs) > 0:\n    total = total + xs.pop(0).unwrap()\nprint(total)?", n * (n - 1) // 2
    for n in (200_000, 800_000):
        # Build query keys once so the lookup loop does not format each query.
        for name, key, prefix in (
            ("hash-string", "keys[i]", ""),
            ("hash-long-string", "keys[i]", "application.configuration.worker.http.client.header.request.name."),
            ("hash-tuple", "(7, keys[i])", ""),
        ):
            keys = f'[f"{prefix}{{i}}"? for i in range(2048)]?'
            query = "(7, keys[i % 2048])" if name == "hash-tuple" else "keys[i % 2048]"
            body = f"keys = {keys}\nxs = {{{key}: i for i in range(1024)}}?\nmut total = 0\nmut i = 0\nwhile i < {n}:\n    if {query} in xs:\n        total = total + 1\n    i = i + 1\nprint(total)?"
            yield name, n, body, 1024 * (n // 2048) + min(1024, n % 2048)
    # All equal-length combinations collide in bare FNV-1a's low four bits.
    # This is a reproducible structured input, not a cryptographic attack test.
    words = '["a", "q", "A", "Q", "1", "!"]?'
    for n in (2000, 8000):
        keys = f'["".join([words[i % 6], words[(i // 6) % 6], words[(i // 36) % 6], words[(i // 216) % 6], words[(i // 1296) % 6], words[(i // 7776) % 6]]?)? for i in range({n})]?'
        yield "hash-pattern", n, f"words = {words}\nkeys = {keys}\nxs = {{keys[i]: i for i in range({n})}}?\nmut total = 0\nfor key in &keys:\n    total = total + xs[key]\nprint(total)?", n * (n - 1) // 2


def write_source(directory, name, n, body):
    # Default integer arithmetic (`+`, `-`, `//`, `%`) is checked and reports
    # OverflowError[i64] or ZeroDivisionError through main's return type, so
    # the workloads measure the checked operators. Wrapping a body in
    # `unchecked:` reproduces the old trapping programs.
    source = directory / f"{name}-{n}.plenty"
    lines = "".join(f"    {line}\n" for line in body.splitlines())
    source.write_text(f"def main() -> Result[(), AllocError | IoError | OverflowError[i64] | ZeroDivisionError]:\n{lines}    Ok(())\n")
    return source


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--compiler", required=True, type=pathlib.Path)
    parser.add_argument("--baseline-compiler", type=pathlib.Path, help="Compare by alternating execution order each repetition")
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--check-only", action="store_true", help="Type-check each generated source with --check (and the baseline compiler, if given), printing one line per workload; compile and run nothing")
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    compiler = args.compiler.resolve()
    baseline = args.baseline_compiler.resolve() if args.baseline_compiler else None
    args.output.mkdir(parents=True, exist_ok=True)
    if args.check_only:
        for name, n, body, _ in workloads():
            source = write_source(args.output, name, n, body)
            for command in (compiler, baseline) if baseline else (compiler,):
                subprocess.run([command, "--check", source], check=True, stdout=subprocess.DEVNULL)
            print(f"checked {name}-{n}", flush=True)
        return
    metadata = {"compiler": str(compiler), "sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(), "host": platform.platform(), "machine": platform.machine(), "options": "--compile FILE -o OUT", "runs": args.runs, "cpu_affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None, "plenty_cache": os.environ.get("PLENTY_CACHE", "default")}
    if baseline:
        metadata["baseline"] = {"compiler": str(baseline), "sha256": hashlib.sha256(baseline.read_bytes()).hexdigest()}
    if hasattr(os, "getloadavg"):
        metadata["load_average"] = os.getloadavg()
    print(json.dumps(metadata))
    results = []
    for name, n, body, expected in workloads():
        source = write_source(args.output, name, n, body)
        executable = source.with_suffix("")
        variants = [("candidate", compiler, executable)]
        if baseline:
            variants.append(("baseline", baseline, executable.with_name(executable.name + "-baseline")))
        compilation = {}
        timings = {label: [] for label, _, _ in variants}
        for label, command, target in variants:
            start = time.perf_counter()
            subprocess.run([command, "--compile", source, "-o", target], check=True, stdout=subprocess.DEVNULL)
            compilation[label] = time.perf_counter() - start
        for repetition in range(args.runs):
            for label, _, target in variants if repetition % 2 == 0 else reversed(variants):
                start = time.perf_counter()
                output = subprocess.run([target.resolve()], check=True, capture_output=True, text=True)
                timings[label].append(time.perf_counter() - start)
                assert output.stdout.strip() == str(expected), (source, label, output.stdout, expected)
        times = timings["candidate"]
        result = {"workload": name, "size": n, "checksum": expected, "compile_seconds": compilation["candidate"], "run_seconds": times, "median_seconds": statistics.median(times)}
        if baseline:
            result.update(baseline_compile_seconds=compilation["baseline"], baseline_run_seconds=timings["baseline"], baseline_median_seconds=statistics.median(timings["baseline"]))
        results.append(result)
        print(json.dumps(result), flush=True)
    (args.output / "results.json").write_text(json.dumps({"metadata": metadata, "results": results}, indent=2) + "\n")


if __name__ == "__main__":
    main()
