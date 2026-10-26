#!/usr/bin/env python3
"""Measure the C-boundary multiply kernel; optional second library is a control.

Both libraries must export plla_v1_multiply with the same wrapper. Supply an
otherwise identical build against upstream faer to isolate the fork changes.
This excludes Plenty matrix construction and includes native validation and
workspace acquisition. Initial calls warm each library's reusable workspace.
"""
import argparse
import ctypes as c
import os
import statistics
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("libraries", nargs="+")
    parser.add_argument("--sizes", nargs="+", type=int, default=[32, 128, 512])
    parser.add_argument("--cpu", type=int)
    parser.add_argument("--strides", nargs="+", type=int, choices=[8, 16], default=[8, 16])
    parser.add_argument("--iterations", type=int, help="fixed calls per sample, useful with perf stat")
    args = parser.parse_args()
    if any(n <= 0 for n in args.sizes) or (args.iterations is not None and args.iterations <= 0):
        parser.error("sizes and iterations must be positive")
    if args.cpu is not None:
        os.sched_setaffinity(0, {args.cpu})
    functions = []
    for path in args.libraries:
        library = c.CDLL(path)
        function = library.plla_v1_multiply
        function.argtypes = [c.c_int64] * 3 + [c.c_void_p, c.c_size_t, c.c_size_t] * 3
        function.restype = c.c_int64
        functions.append((path, function))
    print("library,n,stride_bytes,median_ms,min_ms")
    for n in args.sizes:
        for stride in args.strides:
            step = stride // 8
            a = (c.c_double * (n*n*step))()
            out = (c.c_double * (n*n*step))()
            for i in range(n*n):
                a[i*step] = 1.0
            for path, function in functions:
                params = (n,n,n,a,n*n,stride,a,n*n,stride,out,n*n,stride)
                assert function(*params) == 0
                assert all(out[i*step] == n for i in range(n*n))
                samples = []
                for _ in range(7):
                    start = time.perf_counter()
                    count = 0
                    while (count < args.iterations if args.iterations is not None
                           else time.perf_counter() - start < 0.08):
                        assert function(*params) == 0
                        count += 1
                    samples.append((time.perf_counter()-start)*1000/count)
                print(f"{path},{n},{stride},{statistics.median(samples):.6f},{min(samples):.6f}", flush=True)


if __name__ == "__main__":
    main()
