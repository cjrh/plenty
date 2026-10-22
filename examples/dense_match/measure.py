"""Generate and measure dense match chains and a Plenty decision-tree control.

Only the result JSON is intended for version control; generated sources, dumps,
and executables are retained in --output so each measurement can be inspected.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import statistics
import subprocess
import time


MASK = (1 << 64) - 1


def value(i):
    return ((i + 1) * 2654435761) % 100003


def tree(lo, hi, indent=4):
    pad = " " * indent
    if hi - lo == 1:
        return f"{pad}{value(lo)}u64\n"
    mid = (lo + hi) // 2
    return (f"{pad}if x < {mid}u64:\n" + tree(lo, mid, indent + 4)
            + f"{pad}else:\n" + tree(mid, hi, indent + 4))


def source(n, kind, distribution, iterations):
    enum = kind == "enum"
    text = ""
    if enum:
        text += "enum Choice:\n" + "".join(f"    V{i}\n" for i in range(n)) + "\n"
    text += f"def dispatch(x: {'&Choice' if enum else 'u64'}) -> u64:\n"
    if kind == "tree":
        text += f"    if x >= {n}u64:\n        return 0u64\n" + tree(0, n)
    else:
        text += "    match x:\n"
        for i in range(n):
            pat = f"Choice.V{i}" if enum else f"{i}u64"
            text += f"        case {pat}:\n            {value(i)}u64\n"
        if not enum:
            text += "        case _:\n            0u64\n"
    text += "\ndef main() -> Result[i32, AllocError | IoError]:\n"
    if enum:
        text += "    choices = [" + ", ".join(f"Choice.V{i}" for i in range(n)) + "]?\n"
    # Validate every arm independently of the timed pseudo-random sequence.
    for i in range(n):
        arg = f"&choices[{i}]" if enum else f"{i}u64"
        text += f"    if dispatch({arg}) != {value(i)}u64:\n        return Ok(1i32)\n"
    if not enum:
        text += f"    if dispatch({n}u64) != 0u64 or dispatch(u64.MAX) != 0u64:\n        return Ok(1i32)\n"
    text += "    mut state = 42u64\n    mut total = 0u64\n    mut i = 0u64\n"
    text += f"    while i < {iterations}u64:\n"
    text += "        state = state.wrapping_mul(6364136223846793005u64).wrapping_add(1442695040888963407u64)\n"
    text += f"        mut index = (state >> 32u64) & {n - 1}u64\n"
    if distribution != "uniform":
        hot = 0 if distribution == "first" else n - 1
        text += f"        if state % 10u64 != 0u64:\n            index = {hot}u64\n"
    arg = "&choices[i64(index)]" if enum else "index"
    text += f"        total = total.wrapping_add(dispatch({arg}))\n"
    text += "        i = i.wrapping_add(1u64)\n    print(total)?\n    Ok(0i32)\n"
    return text


def expected(n, distribution, iterations):
    state = 42
    total = 0
    for _ in range(iterations):
        state = (state * 6364136223846793005 + 1442695040888963407) & MASK
        index = (state >> 32) & (n - 1)
        if distribution != "uniform" and state % 10 != 0:
            index = 0 if distribution == "first" else n - 1
        total = (total + value(index)) & MASK
    return str(total) + "\n"


def run(command):
    start = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True, timeout=180)
    elapsed = time.perf_counter() - start
    if result.returncode:
        raise RuntimeError(f"{command}:\n{result.stdout}{result.stderr}")
    return result.stdout, elapsed


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cpu", type=int, required=True)
    parser.add_argument("--sizes", type=int, nargs="+", default=[4, 16, 64, 256])
    parser.add_argument("--iterations", type=int, default=2_000_000)
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--compile-repeats", type=int, default=3)
    parser.add_argument("--baseline-output", type=Path,
                        help="Interleave runtime reruns of an earlier baseline output directory")
    args = parser.parse_args()
    if any(n < 2 or n & (n - 1) for n in args.sizes):
        parser.error("sizes must be powers of two >= 2")
    if min(args.iterations, args.repeats, args.compile_repeats) < 1:
        parser.error("iteration and repetition counts must be positive")
    os.sched_setaffinity(0, {args.cpu})
    compiler = args.compiler.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    root = Path(__file__).resolve().parents[2]
    revision = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    report = {"revision": revision, "compiler": str(compiler),
              "measured_at": datetime.now(timezone.utc).isoformat(),
              "switch_experiment": "PLENTY_DENSE_SWITCH" in os.environ,
              "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
              "platform": platform.platform(), "cpu": args.cpu,
              "iterations": args.iterations, "repeats": args.repeats,
              "compile_repeats": args.compile_repeats, "rows": []}
    jobs = []
    for n in args.sizes:
        for distribution in ("uniform", "first", "last"):
            checksum = expected(n, distribution, args.iterations)
            for kind in ("integer", "tree", "enum"):
                name = f"{kind}-{n}-{distribution}"
                path = output / (name + ".plenty")
                path.write_text(source(n, kind, distribution, args.iterations))
                binary = output / name
                command = [str(compiler), "--release", "--no-cache", "--compile", str(path), "-o", str(binary)]
                row = {"name": name, "arms": n, "kind": kind, "distribution": distribution,
                       "variant": "switch" if report["switch_experiment"] else "baseline",
                       "source_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                       "compile_command": command, "compile_seconds": [], "run_seconds": [],
                       "checksum": checksum.strip()}
                for _ in range(args.compile_repeats):
                    row["compile_seconds"].append(run(command)[1])
                if distribution == "uniform":
                    dump = output / (name + "-dump")
                    run([str(compiler), "--release", "--dump", "clif-opt,asm", "--dump-filter", "dispatch", str(path), "-o", str(dump)])
                    row["dump_index"] = (dump / "index.txt").read_text()
                stdout, _ = run([str(binary)])  # Warm-up, also checks every arm.
                if stdout != checksum:
                    raise AssertionError((name, stdout, checksum))
                jobs.append((binary, checksum, row))
                report["rows"].append(row)
    if args.baseline_output:
        previous = json.loads((args.baseline_output / "results.json").read_text())
        if previous["iterations"] != args.iterations:
            raise ValueError("baseline iteration count differs")
        current = {row["name"]: row for row in report["rows"]}
        for row in previous["rows"]:
            if row["name"] not in current:
                continue
            if row["source_sha256"] != current[row["name"]]["source_sha256"]:
                raise ValueError(f"baseline source differs: {row['name']}")
            row["variant"] = "baseline"
            row["run_seconds"] = []
            jobs.append((args.baseline_output.resolve() / row["name"], row["checksum"] + "\n", row))
            report["rows"].append(row)
        report["baseline_compiler_sha256"] = previous["compiler_sha256"]
    # Shuffle each round reproducibly to avoid placing one implementation at a
    # systematically different temperature/frequency than its alternatives.
    rng = random.Random(473)
    for _ in range(args.repeats):
        rng.shuffle(jobs)
        for binary, checksum, row in jobs:
            stdout, elapsed = run([str(binary)])
            if stdout != checksum:
                raise AssertionError((binary, stdout, checksum))
            row["run_seconds"].append(elapsed)
    for row in report["rows"]:
        row["compile_median_seconds"] = statistics.median(row["compile_seconds"])
        row["run_median_seconds"] = statistics.median(row["run_seconds"])
        print(f"{row['variant']:8} {row['name']:24} compile={row['compile_median_seconds']:.3f}s run={row['run_median_seconds']:.4f}s", flush=True)
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
