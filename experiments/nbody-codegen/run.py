#!/usr/bin/env python3
"""Reproduce the instruction/assembly investigation without changing baselines."""
import argparse
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def run(command, **kwargs):
    return subprocess.run([str(arg) for arg in command], check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--steps', type=int, default=1_000_000)
    parser.add_argument('--work', type=Path, default=ROOT / 'target/nbody-codegen')
    args = parser.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    compiler = ROOT / 'target/release/plenty'
    source = ROOT / 'benchmarks/game/n-body'
    results = {}
    env = dict(os.environ, PLENTY_CACHE='off')
    for name, flags in {
        'rust-native': ['-C', 'target-cpu=native'],
        'rust-baseline': ['-C', 'target-cpu=x86-64'],
        'rust-native-scalar': ['-C', 'target-cpu=native', '-C', 'no-vectorize-loops', '-C', 'no-vectorize-slp'],
        'rust-baseline-scalar': ['-C', 'target-cpu=x86-64', '-C', 'no-vectorize-loops', '-C', 'no-vectorize-slp'],
    }.items():
        run(['rustc', '-C', 'opt-level=3', '-C', 'codegen-units=1', *flags,
             '--emit=llvm-ir,asm,link', source / 'reference/nbody.rs', '-o', work / name])
    for name, filename, flags in [
        ('plenty', 'nbody.plenty', []),
        ('plenty-clist', 'nbody_clist.plenty', []),
    ]:
        run([compiler, '--compile', '--no-strip', *flags, source / 'plenty' / filename, '-o', work / name], env=env)
    for name in ['rust-native', 'rust-baseline', 'rust-native-scalar', 'rust-baseline-scalar', 'plenty', 'plenty-clist']:
        counts = []
        for repeat in range(3):
            counter = work / f'{name}-{repeat}.perf'
            with (work / f'{name}.out').open('w') as output:
                run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', counter,
                     'taskset', '-c', '0', work / name, args.steps], stdout=output)
            counts.append(next(int(line.split(';')[0]) for line in counter.read_text().splitlines()
                               if ';instructions:u;' in line))
        results[name] = counts
    expected = [float(x) for x in (work / 'rust-native.out').read_text().split()]
    for name in results:
        actual = [float(x) for x in (work / f'{name}.out').read_text().split()]
        if len(actual) != len(expected) or any(abs(x-y) > 1e-8 for x,y in zip(actual, expected)):
            raise RuntimeError(f'{name}: output mismatch: {actual} vs {expected}')
    result = {'steps': args.steps, 'instructions': results,
              'commit': subprocess.check_output(['git', '-C', ROOT, 'rev-parse', 'HEAD'], text=True).strip()}
    (work / 'counts.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
    for kernel in ('magnitude', 'velocity'):
        run([compiler, '--dump', 'clif-opt,asm', '--dump-filter', kernel,
             ROOT / f'experiments/nbody-codegen/{kernel}.plenty', '-o', work / kernel], env=env)
        for opt in ('O2', 'O3'):
            obj = work / f'{kernel}-{opt}.o'
            run(['gcc', f'-{opt}', '-fno-math-errno', '-ffp-contract=off',
                 '-fno-tree-loop-distribute-patterns', '-c',
                 ROOT / f'experiments/nbody-codegen/{kernel}.c', '-o', obj])
            with (work / f'{kernel}-{opt}.asm').open('w') as output:
                run(['objdump', '-d', '--no-show-raw-insn', obj], stdout=output)
    with (work / 'magnitude-clif.txt').open('w') as output:
        run([ROOT / 'target/release/plenty-kernel-harness',
             ROOT / 'experiments/nbody-codegen/magnitude.clif'], stdout=output)


if __name__ == '__main__':
    main()
