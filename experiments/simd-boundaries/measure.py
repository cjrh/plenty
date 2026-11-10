#!/usr/bin/env python3
"""Compare retained before/after binaries; run from the worktree root."""
import json
from pathlib import Path
import statistics
import subprocess
import time

WORK = Path('target/simd-boundaries')

def run(args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, **kwargs)


def main():
    names = ['projection', 'simd-xy', 'simd-locals', 'original']
    verified = 0
    for name in names:
        for steps in [-1, 0, 1, 2, 3, 10, 1000, 10000, 1000000]:
            suffix = [] if name == 'projection' else ['state']
            before = run([WORK / f'{name}-before', steps, *suffix], capture_output=True, text=True).stdout
            after = run([WORK / f'{name}-after', steps, *suffix], capture_output=True, text=True).stdout
            assert before == after, (name, steps)
            if name == 'projection':
                c = run([WORK / 'projection-c', steps], capture_output=True, text=True).stdout
                assert float(c).hex() == float(after).hex(), (name, steps, c, after)
            verified += 1
    result = {'baseline': '56365416', 'steps': 1000000, 'verified_program_step_cases': verified, 'instructions': {}, 'seconds': {}, 'compile_instructions': {}}
    bins = [f'{name}-{version}' for name in names for version in ['before', 'after']] + ['projection-c']
    for binary in bins:
        counts = []
        for repeat in range(3):
            counter = WORK / f'{binary}-{repeat}.perf'
            run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', counter, 'taskset', '-c', '0', WORK / binary, 1000000], stdout=subprocess.DEVNULL)
            counts.append(next(int(line.split(';')[0]) for line in counter.read_text().splitlines() if ';instructions:u;' in line))
        result['instructions'][binary] = counts
        print(binary, statistics.median(counts), flush=True)
    for repeat in range(9):
        for binary in bins[repeat:] + bins[:repeat]:
            start = time.perf_counter()
            run(['taskset', '-c', '0', WORK / binary, 1000000], stdout=subprocess.DEVNULL)
            result['seconds'].setdefault(binary, []).append(time.perf_counter() - start)
    for name, source in [('projection', Path('experiments/simd-boundaries/projection.plenty')), ('simd-xy', WORK / 'nbody/simd-xy.plenty'), ('original', WORK / 'nbody/original.plenty')]:
        for version, compiler in [('before', WORK / 'plenty-before'), ('after', Path('target/release/plenty'))]:
            counts = []
            for repeat in range(3):
                counter = WORK / f'compile-{name}-{version}-{repeat}.perf'
                run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', counter, 'taskset', '-c', '0', compiler, '--no-cache', '--compile', source, '-o', WORK / 'compile-output'], stdout=subprocess.DEVNULL)
                counts.append(next(int(line.split(';')[0]) for line in counter.read_text().splitlines() if ';instructions:u;' in line))
            result['compile_instructions'][f'{name}-{version}'] = counts
    (WORK / 'counts.json').write_text(json.dumps(result, indent=2) + '\n')

if __name__ == '__main__':
    main()
