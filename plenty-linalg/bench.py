#!/usr/bin/env python3
"""Measure the C-boundary multiply kernel; optional second library is a control.

Both libraries must export plla_v1_multiply with the same wrapper. Supply an
otherwise identical build against upstream faer to isolate the vendor patch.
This excludes Plenty allocation/construction and includes native validation.
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
    args = parser.parse_args()
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
        for stride in [8, 16]:
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
                    while time.perf_counter() - start < 0.08:
                        assert function(*params) == 0
                        count += 1
                    samples.append((time.perf_counter()-start)*1000/count)
                print(f"{path},{n},{stride},{statistics.median(samples):.6f},{min(samples):.6f}", flush=True)


if __name__ == "__main__":
    main()
