#!/usr/bin/env python3
"""Prepare/verify actual Plenty Vec3 programs, then measure in a quiet slot.

No kernel bytes or hand-written CLIF are injected. --measure reuses the exact
prepared executables and records hashes, so builds and timing stay separate.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def run(args, **kwargs):
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work', type=Path, default=ROOT / 'target/vec3-source/final')
    parser.add_argument('--compiler', type=Path, default=ROOT / 'target/release/plenty')
    parser.add_argument('--measure', action='store_true')
    parser.add_argument('--core', default='3')
    parser.add_argument('--steps', type=int, default=1_000_000)
    args = parser.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    manifest = work / 'preparation.json'
    if not args.measure:
        source = module('vec3_source', HERE / 'source.py')
        source.generate(work)
        source.old.c_locals(work)
        loops = module('loops', ROOT / 'experiments/kernels/loops.py')
        binaries, counts, checks = {}, {}, []
        variants = [('production-v3', 'production', 'x86-64-v3', False),
                    ('production-baseline', 'production', 'x86-64', False),
                    ('scalar-v3', 'array-scalar', 'x86-64-v3', False),
                    ('ordinary-v3', 'array-ordinary', 'x86-64-v3', False),
                    ('vec3-v3', 'array-vec3', 'x86-64-v3', False),
                    ('vec3-unpaired-v3', 'array-vec3-unpaired', 'x86-64-v3', False),
                    ('scalar-baseline', 'array-scalar', 'x86-64', False),
                    ('vec3-baseline', 'array-vec3', 'x86-64', False),
                    ('vec3-fast', 'array-vec3', 'x86-64', True)]
        for name, stem, cpu, fast in variants:
            path, executable = work / (stem + '.plenty'), work / name
            flags = ['--cpu', cpu] + (['--fast-compile'] if fast else [])
            run([args.compiler, '--compile', '--no-cache', *flags, path, '-o', executable])
            dump = work / (name + '-dump')
            run([args.compiler, '--dump', 'asm,clif-opt', *flags, '--dump-filter', 'advance', path, '-o', dump])
            assembly = (dump / 'asm.txt').read_text()
            assert not any(op in assembly for op in ('vfmadd', 'vfmsub', 'vfnmadd', 'vfnmsub')), name
            if name == 'vec3-v3':
                assert 'rebuilt-narrow' not in assembly and '%ymm' in assembly
                assert assembly.count('vsqrtpd') == assembly.count('vdivpd') == 5
            with (dump / 'asm.txt').open() as asm:
                counts[name] = [{'function': f, 'start': s, 'stop': e, 'instructions': len(i)}
                                for f, s, e, i in loops.loops(loops.parse(asm))]
            binaries[name] = executable
        binaries['c-v3'] = work / 'c-v3'
        run(['gcc', '-O3', '-march=x86-64-v3', '-ffp-contract=off', '-fno-math-errno',
             '-fno-tree-vectorize', '-DKERNEL=locals', ROOT / 'experiments/nbody-structural/driver.c',
             work / 'locals.c', '-lm', '-o', binaries['c-v3']])
        # Fast-compile's independent scalar result participates in correctness;
        # its intentionally slower runtime is excluded from adoption timings.
        for steps in (-1, 0, 1, 2, 3, 10, 1000, 10000, args.steps):
            expected = run([binaries['scalar-v3'], steps, 'state'], capture_output=True, text=True).stdout.split()
            assert len(expected) == 37
            for name, executable in binaries.items():
                actual = run([executable, steps, 'state'], capture_output=True, text=True).stdout.split()
                assert len(actual) == 37 and [float(x).hex() for x in actual] == [float(x).hex() for x in expected], (name, steps)
                checks.append([name, steps])
        boundary_checks = []
        boundary_counts = {}
        for kind in ('local', 'owned', 'shared', 'array'):
            name = 'cost-' + kind
            path, executable = work / (name + '.plenty'), work / name
            run([args.compiler, '--compile', '--no-cache', '--cpu', 'x86-64-v3', path, '-o', executable])
            dump = work / (name + '-dump')
            run([args.compiler, '--dump', 'asm,clif-opt', '--cpu', 'x86-64-v3',
                 '--dump-filter', 'advance', path, '-o', dump])
            with (dump / 'asm.txt').open() as asm:
                boundary_counts[name] = [{'function': f, 'start': s, 'stop': e, 'instructions': len(i)}
                                        for f, s, e, i in loops.loops(loops.parse(asm))]
            binaries[name] = executable
        for steps in (-1, 0, 1, 2, 3, 10, 1000, 10000):
            expected = run([binaries['cost-local'], steps], capture_output=True).stdout
            for kind in ('local', 'owned', 'shared', 'array'):
                name = 'cost-' + kind
                assert run([binaries[name], steps], capture_output=True).stdout == expected, (name, steps)
                boundary_checks.append([name, steps])
        sources = [HERE / 'source.py', HERE / 'source_measure.py', ROOT / 'experiments/nbody-structural/generate.py',
                   ROOT / 'experiments/nbody-structural/driver.c', ROOT / 'benchmarks/game/n-body/plenty/nbody.plenty',
                   ROOT / 'stdlib/std/vector.plenty',
                   ROOT / 'src/codegen/portable_simd.rs', ROOT / 'src/codegen.rs', ROOT / 'src/codegen/enums.rs',
                   ROOT / 'src/frontend/modules.rs', ROOT / 'Cargo.lock']
        data = {'commit': subprocess.check_output(['git', '-C', ROOT, 'rev-parse', 'HEAD'], text=True).strip(),
                'compiler': str(args.compiler.resolve()), 'compiler_sha256': digest(args.compiler),
                'sources': {str(p.relative_to(ROOT)): digest(p) for p in sources},
                'binaries': {n: {'path': str(p), 'sha256': digest(p)} for n, p in binaries.items()},
                'loops': counts, 'verification': checks,
                'boundary_loops': boundary_counts, 'boundary_verification': boundary_checks, 'state_values': 35, 'energy_values': 2,
                'storage': {'vec3_payload': 48, 'vec3_owning_slot': 64, 'body_payload': 112,
                            'body_owning_slot': 128, 'five_body_array_logical_bytes': 560}}
        manifest.write_text(json.dumps(data, indent=2) + '\n')
        print(f'{len(checks)} complete-state checks passed; preparation saved to {manifest}')
        return
    data = json.loads(manifest.read_text())
    binaries = {name: Path(item['path']) for name, item in data['binaries'].items() if name != 'vec3-fast'}
    for name, path in binaries.items():
        assert digest(path) == data['binaries'][name]['sha256'], name
    results = {'steps': args.steps, 'core': args.core, 'samples': {}, 'preparation_sha256': digest(manifest), 'measurement_driver_sha256': digest(Path(__file__))}
    names = list(binaries)
    for repeat in range(9):
        for name in names[repeat % len(names):] + names[:repeat % len(names)]:
            counter = work / f'{name}-{repeat}.perf'
            run(['perf', 'stat', '-x;', '-e', 'instructions:u,cycles:u,task-clock', '-o', counter,
                 'taskset', '-c', args.core, binaries[name], args.steps], stdout=subprocess.DEVNULL)
            sample = {}
            for line in counter.read_text().splitlines():
                fields = line.split(';')
                if len(fields) > 2 and fields[2] in ('instructions:u', 'cycles:u', 'task-clock', 'task-clock:u'):
                    sample['task-clock' if fields[2] == 'task-clock:u' else fields[2]] = float(fields[0])
            assert len(sample) == 3, counter
            results['samples'].setdefault(name, []).append(sample)
    results['medians'] = {n: {metric: statistics.median(row[metric] for row in rows)
                              for metric in rows[0]} for n, rows in results['samples'].items()}
    (work / 'measurements.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(results['medians'], indent=2))


if __name__ == '__main__':
    main()
