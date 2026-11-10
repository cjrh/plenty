#!/usr/bin/env python3
"""Build, verify, and measure n-body source/IR structural experiments.

Run after building this worktree's release compiler and kernel harness.
Only generated output under --work is changed. No benchmark is replaced.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def run(args, **kw):
    return subprocess.run([str(x) for x in args], check=True, **kw)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--work', type=Path, default=ROOT / 'target/nbody-structural')
    p.add_argument('--steps', type=int, default=1_000_000)
    p.add_argument('--build-only', action='store_true')
    args = p.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PLENTY_CACHE='off')
    run(['python3', '-B', HERE / 'generate.py', work])
    run(['python3', '-B', HERE / 'ir.py', work])
    bins = {}
    for name in ('original', 'fused', 'locals', 'simd-locals', 'simd-xy'):
        output = work / ('plenty-' + name)
        run([ROOT / 'target/release/plenty', '--check', work / f'{name}.plenty'], env=env)
        run([ROOT / 'target/release/plenty', '--compile', '--no-strip', work / f'{name}.plenty', '-o', output], env=env)
        bins['plenty-' + name] = output
        run([ROOT / 'target/release/plenty', '--dump', 'clif-opt,asm', '--dump-filter', 'advance',
             work / f'{name}.plenty', '-o', work / f'{name}-dump'], env=env)
    flags = ['-O3', '-march=x86-64', '-fno-math-errno', '-ffp-contract=off']
    # Driver stays scalar and identical; all optimization ablations affect only
    # kernels, compiled as separate translation units (no LTO).
    for label, cc, extra in [('gcc', 'gcc', []), ('clang', 'clang', []),
                             ('clang-scalar', 'clang', ['-fno-vectorize', '-fno-slp-vectorize']),
                             ('clang-native', 'clang', ['-march=native'])]:
        for shape in ('staged', 'fused', 'locals'):
            kernel_source = work / 'locals.c' if shape == 'locals' else HERE / 'kernels.c'
            obj = work / f'{label}-{shape}.o'
            run([cc, *flags, *extra, '-c', kernel_source, '-o', obj])
            name = f'{label}-{shape}'
            bins[name] = work / name
            run(['gcc', *flags, '-fno-tree-vectorize', f'-DKERNEL={shape}', HERE / 'driver.c', obj, '-lm', '-o', bins[name]])
            with (work / f'{name}.asm').open('w') as f:
                run(['objdump', '-d', '--no-show-raw-insn', obj], stdout=f)
    for isa in ('baseline', 'avx'):
        ir_env = dict(env, PLENTY_KERNEL_CODE_DIR=str(work / ('raw-' + isa)))
        ir_env.pop('PLENTY_KERNEL_AVX', None)
        if isa == 'avx':
            ir_env['PLENTY_KERNEL_AVX'] = '1'
        with (work / f'ir-{isa}.asm').open('w') as f:
            run([ROOT / 'target/release/plenty-kernel-harness', work / 'scalar.clif', work / 'xy.clif', work / 'xy-pairs.clif'],
                env=ir_env, stdout=f)
        for i, shape in enumerate(('scalar', 'xy', 'xy-pairs')):
            name = f'clif-{isa}-{shape}'
            assembly = work / f'{name}.s'
            assembly.write_text('.text\n.p2align 4\n.globl advance\n.type advance,@function\nadvance:\n'
                f'.incbin "{work / ("raw-" + isa) / f"{i}-0.bin"}"\n'
                '.size advance, .-advance\n.section .note.GNU-stack,"",@progbits\n')
            bins[name] = work / name
            run(['gcc', *flags, '-fno-tree-vectorize', '-DKERNEL=advance', HERE / 'driver.c', assembly, '-lm', '-o', bins[name]])
    for shape in ('baseline', 'native'):
        name = f'rust-{shape}'
        bins[name] = work / name
        run(['rustc', '-C', 'opt-level=3', '-C', 'codegen-units=1', '-C',
             'target-cpu=' + ('x86-64' if shape == 'baseline' else 'native'),
             '--emit=llvm-ir,asm,link', ROOT / 'benchmarks/game/n-body/reference/nbody.rs', '-o', bins[name]])
    reference_obj = work / 'verify-reference.o'
    run(['gcc', *flags, '-fno-tree-vectorize', '-c', HERE / 'kernels.c', '-o', reference_obj])
    for name in bins:
        if name.startswith('clif-') or (name.endswith('-locals') and not name.startswith('plenty-')):
            kernel = 'advance' if name.startswith('clif-') else 'locals'
            source = work / (name + ('.s' if name.startswith('clif-') else '.o'))
            executable = work / (name + '-verify')
            run(['gcc', *flags, '-fno-tree-vectorize', f'-DKERNEL={kernel}', HERE / 'verify.c',
                 source, reference_obj, '-lm', '-o', executable])
            run([executable])
    (work / 'binaries.json').write_text(json.dumps({k:str(v) for k,v in bins.items()}, indent=2)+'\n')
    verification = []
    for steps in (-1, 0, 1, 2, 3, 10, 1000, 10000, args.steps):
        expected = run([bins['plenty-original'], steps, 'state'], capture_output=True, text=True).stdout.split()
        for name, binary in bins.items():
            if name.startswith('rust-'):
                # Rust's parsing/default and initial momentum expression differ.
                if steps < 0:
                    continue
                actual = run([binary, steps], capture_output=True, text=True).stdout.split()
                assert all(abs(float(x)-float(y)) <= 1e-8 for x,y in zip(actual,expected[:2])), (name, steps)
            else:
                actual = run([binary, steps, 'state'], capture_output=True, text=True).stdout.split()
                assert len(actual) == len(expected) == 37, (name, steps, len(actual))
                assert [float(x).hex() for x in actual] == [float(x).hex() for x in expected], (name, steps, actual, expected)
            verification.append([name, steps])
    (work / 'verification.json').write_text(json.dumps({
        'checked_program_step_cases': len(verification),
        'steps': [-1, 0, 1, 2, 3, 10, 1000, 10000, args.steps],
        'state_comparison': '35 floats, exact float.hex; also two energy lines',
        'rust_comparison': 'two energy lines within 1e-8; negative steps excluded',
        'randomized_kernel_cases': '256 per kernel; 6 CLIF and 4 C local kernels; memcmp',
        'programs': list(bins)}, indent=2)+'\n')
    print(f'Verified {len(verification)} cases; complete state bit-identical for Plenty/C/CLIF', flush=True)
    if args.build_only:
        return
    results = {'commit': subprocess.check_output(['git', '-C', ROOT, 'rev-parse', 'HEAD'], text=True).strip(),
               'steps': args.steps, 'instructions': {}, 'timings': {}, 'verification_cases': len(verification)}
    for name, binary in bins.items():
        counts = []
        for repeat in range(3):
            counter = work / f'{name}-{repeat}.perf'
            run(['perf', 'stat', '-x', ';', '-e', 'instructions:u', '-o', counter,
                 'taskset', '-c', '0', binary, args.steps], stdout=subprocess.DEVNULL)
            counts.append(next(int(line.split(';')[0]) for line in counter.read_text().splitlines() if ';instructions:u;' in line))
        results['instructions'][name] = counts
        print(name, statistics.median(counts), flush=True)
    # Rotate order so compiler variant is not coupled to position in each round.
    names = list(bins)
    for repeat in range(9):
        for name in names[repeat:] + names[:repeat]:
            start = time.perf_counter()
            run(['taskset', '-c', '0', bins[name], args.steps], stdout=subprocess.DEVNULL)
            elapsed = time.perf_counter() - start
            results['timings'].setdefault(name, []).append(elapsed)
    (work / 'counts.json').write_text(json.dumps(results, indent=2)+'\n')


if __name__ == '__main__':
    main()
