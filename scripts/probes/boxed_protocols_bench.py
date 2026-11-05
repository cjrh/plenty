#!/usr/bin/env python3
"""Measure construction, shared dispatch and cleanup for four owner representations.

Pass a compiler built from this checkout. Native outputs use --release. Timings
are medians in nanoseconds from std.time, excluding compiler and process startup.
This is a local microbenchmark, not a cross-platform performance guarantee.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

DECLARATIONS = """
from std.time import monotonic_now
protocol Readable:
    def read(self) -> i64:
        pass
class A:
    n: i64
    def read(self) -> i64:
        self.n
class B:
    n: i64
    extra: i64
    def read(self) -> i64:
        self.n + self.extra
enum Item:
    A(A)
    B(B)
    def read(self) -> i64:
        match self:
            case Item.A(value):
                value.read()
            case Item.B(value):
                value.read()
def erase[T: Readable](value: T) -> Result[BoxedCallable[[], i64], AllocError]:
    callback = def [value]() -> i64:
        value.read()
    BoxedCallable(callback)
"""


def source(kind, count, rounds):
    ty, first, second, call = {
        "inline_enum": ("Item", "Item.A(A(n))", "Item.B(B(n, 1))", "value.read()"),
        "concrete_box": ("Box[Item]", "Box(Item.A(A(n)))?", "Box(Item.B(B(n, 1)))?", "value.read()"),
        "boxed_callable": ("BoxedCallable[[], i64]", "erase(A(n))?", "erase(B(n, 1))?", "value()"),
        "boxed_protocol": ("Box[Readable]", "Box[Readable](A(n))?", "Box[Readable](B(n, 1))?", "value.read()"),
    }[kind]
    return DECLARATIONS + f"""
def main() -> Result[(), AllocError | IoError | RangeError]:
    start = monotonic_now()
    mut values: list[{ty}] = []?
    for n in range({count}):
        if n % 2 == 0:
            values.append({first})?
        else:
            values.append({second})?
    constructed = monotonic_now()
    mut total = 0
    for repeat in range({rounds}):
        for value in &values:
            total += {call}
    read = monotonic_now()
    drop(values)
    finished = monotonic_now()
    print(constructed.duration_since(&start)?.as_nanoseconds(), read.duration_since(&constructed)?.as_nanoseconds(), finished.duration_since(&read)?.as_nanoseconds(), total)?
    Ok(())
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--count", type=int, default=20000)
    parser.add_argument("--rounds", type=int, default=200)
    parser.add_argument("--samples", type=int, default=5)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    if min(args.count, args.rounds, args.samples) < 1:
        parser.error("counts must be positive")
    expected = (args.count * (args.count - 1) // 2 + args.count // 2) * args.rounds
    results = {}
    with tempfile.TemporaryDirectory(prefix="plenty-protocol-bench-") as temp:
        for kind in ["inline_enum", "concrete_box", "boxed_callable", "boxed_protocol"]:
            path = Path(temp) / f"{kind}.plenty"
            binary = Path(temp) / kind
            path.write_text(source(kind, args.count, args.rounds))
            start = time.perf_counter()
            subprocess.run([str(compiler), "--release", "--compile", str(path), "-o", str(binary)], check=True, capture_output=True, env={**os.environ, "PLENTY_CACHE": "off"})
            compile_seconds = time.perf_counter() - start
            timings = []
            for _ in range(args.samples):
                result = subprocess.run([str(binary)], capture_output=True, text=True, check=True)
                construction, dispatch, cleanup, total = map(int, result.stdout.split())
                if total != expected:
                    raise RuntimeError(f"{kind}: wrong checksum {total}, expected {expected}")
                timings.append((construction, dispatch, cleanup))
            results[kind] = dict(zip(["construct_ns", "dispatch_ns", "cleanup_ns"], map(statistics.median, zip(*timings))))
            results[kind].update(compile_seconds=compile_seconds, executable_bytes=binary.stat().st_size)
    print(json.dumps({"count": args.count, "rounds": args.rounds, "samples": args.samples, "results": results}, indent=2))


if __name__ == "__main__":
    main()
