#!/usr/bin/env python3
"""Reproduce guarded aggregate specialization on unchanged n-body source."""
import argparse
from collections import Counter
import importlib.util
import json
import os
from pathlib import Path
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
ENV = dict(os.environ, PLENTY_CACHE='off')


def run(args, **kw):
    return subprocess.run([str(x) for x in args], check=True, env=ENV, **kw)


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def count(command, log):
    run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', log,
         'taskset', '-c', '0', *command], stdout=subprocess.DEVNULL)
    return next(int(line.split(';')[0]) for line in log.read_text().splitlines()
                if ';instructions:u;' in line)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--before', type=Path, required=True)
    p.add_argument('--after', type=Path, default=ROOT / 'target/release/plenty')
    p.add_argument('--references', type=Path)
    p.add_argument('--work', type=Path, default=ROOT / 'target/fixed-state/measurement')
    args = p.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    run(['python3', '-B', ROOT / 'experiments/nbody-structural/generate.py', work])
    bins = {}
    for name, compiler in [('before', args.before.resolve()), ('after', args.after.resolve())]:
        binary = work / name
        run([compiler, '--compile', '--no-strip', work / 'original.plenty', '-o', binary])
        run([compiler, '--dump', 'clif-opt,asm', '--dump-filter', 'advance',
             work / 'original.plenty', '-o', work / (name + '-dump')])
        bins[name] = binary
    # Differential check complete state, not just the benchmark's rounded energy.
    steps = [-1, 0, 1, 2, 3, 10, 1000, 10000, 1000000]
    for n in steps:
        a = run([bins['before'], n, 'state'], capture_output=True).stdout
        b = run([bins['after'], n, 'state'], capture_output=True).stdout
        assert a == b, n
    if args.references:
        for name in ['rust-baseline', 'rust-native', 'clang-locals', 'clang-native-locals']:
            bins[name] = args.references.resolve() / name
    result = {'baseline': 'c8d502b6b4f738cadb41bbf7ef995974857c4125',
              'steps': 1000000, 'exact_state_steps': steps,
              'instructions': {}, 'seconds': {}, 'compile_instructions': {}, 'loops': {}}
    for name, binary in bins.items():
        result['instructions'][name] = [count([binary, 1000000], work / f'{name}-{r}.perf') for r in range(3)]
    names = list(bins)
    for r in range(9):
        k = r % len(names)
        for name in names[k:] + names[:k]:
            start = time.perf_counter()
            run(['taskset', '-c', '0', bins[name], 1000000], stdout=subprocess.DEVNULL)
            result['seconds'].setdefault(name, []).append(time.perf_counter() - start)
    for name, relative in [('nbody', 'n-body/plenty/nbody.plenty'),
                           ('knucleotide', 'k-nucleotide/plenty/knucleotide.plenty'),
                           ('spectral', 'spectral-norm/plenty/spectralnorm.plenty')]:
        source = ROOT / 'benchmarks/game' / relative
        for label, compiler in [('before', args.before.resolve()), ('after', args.after.resolve())]:
            result['compile_instructions'][name + '-' + label] = [
                count([compiler, '--no-cache', '--compile', source, '-o', work / 'compile-output'],
                      work / f'compile-{name}-{label}-{r}.perf') for r in range(3)]
    loops = module('fixed_state_loops', ROOT / 'experiments/kernels/loops.py')
    ledger = module('fixed_state_ledger', ROOT / 'experiments/nbody-structural/ledger.py')
    for name in ['before', 'after']:
        with (work / (name + '-dump') / 'asm.txt').open() as f:
            rows = list(loops.loops(loops.parse(f)))
        result['loops'][name] = [
            {'name': n, 'start': hex(start), 'end': hex(end), 'instructions': len(body),
             'categories': dict(Counter(ledger.category(m, op) for _, m, op in body))}
            for n, start, end, body in rows]
    (work / 'results.json').write_text(json.dumps(result, indent=2) + '\n')
    for name in names:
        print(name, statistics.median(result['instructions'][name]),
              round(statistics.median(result['seconds'][name]) * 1000, 2), 'ms')


if __name__ == '__main__':
    main()
