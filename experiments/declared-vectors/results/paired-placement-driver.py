#!/usr/bin/env python3
"""Prepare and measure current n-body plus paired/unpaired vector controls.

Preparation and measurement are separate so no compilation runs inside a quiet
measurement window. This is native evidence, not a public representation change.
"""
import argparse
from collections import Counter
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
OLD = ROOT / 'experiments/nbody-structural'
spec = importlib.util.spec_from_file_location('declared_native', HERE / 'native.py')
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)
run = native.run


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def prepare_native(work):
    generated = work / 'generated'
    run(['python3', '-B', HERE / 'ir.py', generated, '--paired'])
    harness = ROOT / 'target/kernel-harness/release/plenty-kernel-harness'
    flags = ['-O2', '-march=x86-64', '-ffp-contract=off', '-fno-tree-vectorize',
             '-fno-tree-loop-distribute-patterns', '-frounding-math']
    reference = work / 'reference.o'
    run(['gcc', *flags, '-c', OLD / 'kernels.c', '-o', reference])
    builds, ledger, checks = {}, {}, []
    env = dict(os.environ)
    env.pop('PLENTY_KERNEL_AVX', None)
    env.pop('PLENTY_KERNEL_AVX2', None)
    for isa, shapes in [('baseline', ['scalar', 'halves']),
                        ('avx2', ['scalar', 'halves', 'wide'])]:
        labels = [shape + suffix for shape in shapes for suffix in ['', '-pairs']]
        raw = work / ('raw-' + isa)
        selected = dict(env, PLENTY_KERNEL_CODE_DIR=str(raw))
        if isa == 'avx2':
            selected['PLENTY_KERNEL_AVX2'] = '1'
        assembly = work / (isa + '.asm')
        with assembly.open('w') as output:
            run([harness, *[generated / ('nbody-' + label + '.clif') for label in labels]],
                env=selected, stdout=output)
        with assembly.open() as source:
            for name, _, _, body in native.source.loops.loops(native.source.loops.parse(source)):
                assert not any('fmadd' in op or 'fmsub' in op for _, op, _ in body), name
                ledger[isa + '-' + name] = {
                    'total': len(body),
                    'categories': dict(Counter(native.source.category(op, args) for _, op, args in body)),
                    'operations': [[op, args] for _, op, args in body],
                }
        for index, label in enumerate(labels):
            name = isa + '-' + label
            asm = work / (name + '.s')
            native.wrapper(asm, 'advance', raw / (str(index) + '-0.bin'))
            binary = work / name
            check = work / (name + '-verify')
            run(['gcc', *flags, '-DKERNEL=advance', OLD / 'driver.c', asm, '-lm', '-o', binary])
            run(['gcc', *flags, HERE / 'verify-nbody-paired.c', asm, reference, '-lm', '-o', check])
            result = run([check], capture_output=True, text=True)
            (work / (name + '-verification.log')).write_text(result.stdout + result.stderr)
            checks.append({'name': name, 'result': result.stdout.strip()})
            builds[name] = {'path': str(binary), 'sha256': digest(binary),
                            'clif_sha256': digest(generated / ('nbody-' + label + '.clif')),
                            'raw_sha256': digest(raw / (str(index) + '-0.bin'))}
    return builds, ledger, checks, digest(harness)


def prepare_source(work):
    source = work / 'source'
    source.mkdir(exist_ok=True)
    run(['python3', '-B', OLD / 'generate.py', source])
    compiler = ROOT / 'target/release/plenty'
    env = dict(os.environ, PLENTY_CACHE='off')
    builds = {}
    for cpu in ['x86-64', 'x86-64-v3']:
        for shape in ['original', 'simd-xy']:
            label = cpu + '-plenty-' + shape
            binary = work / label
            run([compiler, '--no-cache', '--no-strip', '--cpu', cpu, '--compile',
                 source / (shape + '.plenty'), '-o', binary], env=env)
            run([compiler, '--no-cache', '--cpu', cpu, '--dump', 'clif-opt,asm',
                 source / (shape + '.plenty'), '-o', work / (label + '-dump')], env=env)
            builds[label] = {'path': str(binary), 'sha256': digest(binary),
                             'source_sha256': digest(source / (shape + '.plenty'))}
        for shape in ['staged', 'fused']:
            label = cpu + '-c-' + shape
            binary = work / label
            run(['gcc', '-O2', '-march=' + cpu, '-ffp-contract=off', '-DKERNEL=' + shape,
                 OLD / 'driver.c', OLD / 'kernels.c', '-lm', '-o', binary])
            builds[label] = {'path': str(binary), 'sha256': digest(binary)}
    return builds, digest(compiler)


def compare_outputs(builds, steps):
    checks = []
    for count in sorted(set([-1, 0, 1, 2, 3, 1000, steps])):
        expected = None
        for label, artifact in builds.items():
            result = run([artifact['path'], count, 'state'], capture_output=True, text=True)
            actual = [float(value).hex() for value in result.stdout.split()]
            if len(actual) != 37:
                raise RuntimeError(f'{label}: expected full state and energies, got {len(actual)}')
            if expected is None:
                expected = actual
            if actual != expected:
                raise RuntimeError(f'{label}: full-state output differs at {count} steps')
            checks.append({'name': label, 'steps': count,
                           'output_sha256': hashlib.sha256(result.stdout.encode()).hexdigest()})
    return checks


def measure(work, preparation, steps, core):
    builds = preparation['builds']
    for label, artifact in builds.items():
        if digest(Path(artifact['path'])) != artifact['sha256']:
            raise RuntimeError(label + ': executable changed after verification')
    report = {'revision': preparation['revision'], 'steps': steps, 'cpu': core,
              'host': platform.platform(), 'preparation_sha256': digest(work / 'preparation.json'),
              'samples': [], 'medians': {}}
    labels = list(builds)
    for event, rounds in [('instructions:u', 3), ('cycles:u', 9)]:
        for round_number in range(rounds):
            ordered = labels[round_number % len(labels):] + labels[:round_number % len(labels)]
            if round_number % 2:
                ordered.reverse()
            for label in ordered:
                # Paired zero-step controls are retained for process/startup cost.
                for count in [0, steps]:
                    command = ['perf', 'stat', '-x', ';', '-e',
                               'instructions:u,cycles:u,task-clock:u', 'taskset', '-c', str(core),
                               builds[label]['path'], str(count)]
                    before = time.perf_counter()
                    result = run(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
                    elapsed = time.perf_counter() - before
                    counters = {}
                    for line in result.stderr.splitlines():
                        fields = line.split(';')
                        if len(fields) >= 3 and fields[2] in ('instructions:u', 'cycles:u', 'task-clock:u'):
                            counters[fields[2]] = float(fields[0])
                    if len(counters) != 3:
                        raise RuntimeError('missing counter: ' + result.stderr)
                    report['samples'].append({'name': label, 'event': event, 'round': round_number,
                                              'steps': count, 'value': counters[event],
                                              'counters': counters, 'perf_csv': result.stderr,
                                              'command': command,
                                              'perf_invocation_seconds': elapsed})
                    (work / 'measurements.json').write_text(json.dumps(report, indent=2) + '\n')
            print(event, round_number, flush=True)
        for label in labels:
            samples = [s for s in report['samples'] if s['name'] == label and s['event'] == event]
            medians = {str(count): statistics.median(s['value'] for s in samples if s['steps'] == count)
                       for count in [0, steps]}
            report['medians'].setdefault(label, {})[event] = medians
    for label in labels:
        samples = [s for s in report['samples'] if s['name'] == label and s['event'] == 'cycles:u']
        report['medians'][label]['task-clock:u'] = {
            str(count): statistics.median(s['counters']['task-clock:u'] for s in samples if s['steps'] == count)
            for count in [0, steps]}
    (work / 'measurements.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report['medians'], indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work', type=Path, default=ROOT / 'target/declared-vectors-paired')
    parser.add_argument('--steps', type=int, default=1_000_000)
    parser.add_argument('--core', type=int, default=3)
    parser.add_argument('--native-only', action='store_true', help='prepare native controls without source compiler')
    parser.add_argument('--measure-only', action='store_true', help='only measure already verified executables')
    args = parser.parse_args()
    if args.steps <= 0 or args.core < 0:
        parser.error('steps must be positive and core nonnegative')
    if args.native_only and args.measure_only:
        parser.error('--native-only and --measure-only are separate phases')
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        parser.error('native diagnostic calling convention requires Linux x86-64')
    if 'avx2' not in Path('/proc/cpuinfo').read_text():
        parser.error('AVX2 host required')
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    if args.measure_only:
        preparation = json.loads((work / 'preparation.json').read_text())
        if not preparation.get('complete'):
            parser.error('run complete preparation before measuring')
        if args.steps not in {check['steps'] for check in preparation['output_checks']}:
            parser.error('prepare and verify this step count before measuring')
        measure(work, preparation, args.steps, args.core)
        return
    builds, ledger, native_checks, harness_hash = prepare_native(work)
    preparation = {'revision': run(['git', '-C', ROOT, 'rev-parse', 'HEAD'], capture_output=True,
                                   text=True).stdout.strip(), 'builds': builds, 'loops': ledger,
                   'native_checks': native_checks, 'harness_sha256': harness_hash,
                   'complete': False,
                   'source_sha256': {str(path.relative_to(ROOT)): digest(path) for path in
                                     [HERE / 'ir.py', HERE / 'paired.py', HERE / 'native.py',
                                      HERE / 'verify-nbody-paired.c', OLD / 'kernels.c', OLD / 'driver.c']},
                   'tool_versions': {tool: run(command, capture_output=True, text=True).stdout.strip()
                                     for tool, command in [('gcc', ['gcc', '--version']),
                                                           ('perf', ['perf', '--version'])]}}
    if not args.native_only:
        sources, compiler_hash = prepare_source(work)
        builds.update(sources)
        preparation['compiler_sha256'] = compiler_hash
        preparation['output_checks'] = compare_outputs(builds, args.steps)
        preparation['complete'] = True
    (work / 'preparation.json').write_text(json.dumps(preparation, indent=2) + '\n')
    print(json.dumps({'loops': {n: v['total'] for n, v in ledger.items()},
                      'native_checks': native_checks, 'complete': preparation['complete']}, indent=2))


if __name__ == '__main__':
    main()
