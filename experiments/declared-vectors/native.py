#!/usr/bin/env python3
"""Build/verify native diagnostic kernels, then compare n-body in one session."""
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
HERE = Path(__file__).resolve().parent
OLD = ROOT / 'experiments/nbody-structural'
spec = importlib.util.spec_from_file_location('measure', HERE / 'measure.py')
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)
run = source.run


def wrapper(path, symbol, raw):
    path.write_text('.text\n.p2align 4\n.globl ' + symbol + '\n.type ' + symbol + ',@function\n' + symbol + ':\n'
                    '.incbin "' + str(raw.resolve()) + '"\n.size ' + symbol + ', .-' + symbol +
                    '\n.section .note.GNU-stack,"",@progbits\n')


def count(binary, steps, core, event):
    before = time.perf_counter()
    output = run(['perf', 'stat', '-x,', '-e', event, '--', 'taskset', '-c', core, binary, steps],
                 stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True).stderr
    line = next(line for line in output.splitlines() if ',' + event + ',' in line)
    return int(line.split(',')[0]), time.perf_counter() - before


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work', type=Path, default=ROOT / 'target/declared-vectors')
    parser.add_argument('--steps', type=int, default=1_000_000)
    parser.add_argument('--count', action='store_true')
    parser.add_argument('--cycles', action='store_true', help='interleave 9 cycle observations after instruction counts')
    parser.add_argument('--core', type=int, default=0)
    args = parser.parse_args()
    if args.steps <= 0:
        parser.error('--steps must be positive (zero and negative cases are checked separately)')
    work = args.work.resolve()
    native = work / 'native'
    native.mkdir(parents=True, exist_ok=True)
    run(['python3', '-B', HERE / 'ir.py', native])
    env = dict(os.environ, PLENTY_CACHE='off')
    env.pop('PLENTY_KERNEL_AVX', None)
    env.pop('PLENTY_KERNEL_AVX2', None)
    harness = ROOT / 'target/kernel-harness/release/plenty-kernel-harness'
    compiler = ROOT / 'target/release/plenty'
    programs = {'recurrence': {}, 'nbody': {}}
    ledger, verification = {}, []
    flags = ['-O2', '-march=x86-64', '-ffp-contract=off', '-fno-tree-loop-distribute-patterns']
    reference = native / 'reference.o'
    run(['gcc', *flags, '-fno-tree-vectorize', '-c', OLD / 'kernels.c', '-o', reference])
    for isa, shapes in [('baseline', ['scalar', 'halves']), ('avx2', ['scalar', 'halves', 'wide'])]:
        native_env = dict(env)
        if isa == 'avx2':
            native_env['PLENTY_KERNEL_AVX2'] = '1'
        for group, prefix, symbol in [('recurrence', '', 'recurrence'), ('nbody', 'nbody-', 'advance')]:
            raw = native / ('raw-' + group + '-' + isa)
            native_env['PLENTY_KERNEL_CODE_DIR'] = str(raw)
            assembly = native / (group + '-' + isa + '.asm')
            with assembly.open('w') as file:
                run([harness, *[native / (prefix + shape + '.clif') for shape in shapes]], env=native_env, stdout=file)
            with assembly.open() as file:
                for name, start, end, body in source.loops.loops(source.loops.parse(file)):
                    categories = Counter(source.category(m, op) for _, m, op in body)
                    ledger[group + '-' + isa + '-' + name] = dict(total=len(body), categories=dict(categories))
            for index, shape in enumerate(shapes):
                label = 'native-' + isa + '-' + shape
                asm = native / (group + '-' + label + '.s')
                wrapper(asm, symbol, raw / (str(index) + '-0.bin'))
                executable = native / (group + '-' + label)
                check = native / (group + '-' + label + '-verify')
                if group == 'recurrence':
                    run(['gcc', *flags, HERE / 'recurrence-driver.c', asm, '-lm', '-o', executable])
                    run(['gcc', *flags, '-frounding-math', HERE / 'verify-recurrence.c', asm, '-lm', '-o', check])
                else:
                    run(['gcc', *flags, '-DKERNEL=advance', OLD / 'driver.c', asm, '-lm', '-o', executable])
                    run(['gcc', *flags, '-DKERNEL=advance', OLD / 'verify.c', asm, reference, '-lm', '-o', check])
                verified = run([check], capture_output=True, text=True).stdout.strip()
                verification.append([group, label, verified])
                programs[group][label] = executable
    # Reuse the retained source generators; no production benchmark is replaced.
    nbody = work / 'nbody-source'
    nbody.mkdir(exist_ok=True)
    run(['python3', '-B', OLD / 'generate.py', nbody])
    for shape in ['original', 'fused', 'locals', 'simd-xy']:
        text = (nbody / (shape + '.plenty')).read_text()
        fixed = text.replace('list[Body]', 'array[Body, 5]')
        fixed = fixed.replace('starting_state() -> Result[array[Body, 5], AllocError]',
                              'starting_state() -> array[Body, 5]')
        fixed = fixed.replace('starting_state()?', 'starting_state()')
        (nbody / ('array-' + shape + '.plenty')).write_text(fixed)
        for cpu in ['x86-64', 'x86-64-v3']:
            for storage, program in [('list', shape), ('array', 'array-' + shape)]:
                label = cpu + '-' + storage + '-' + shape
                binary = nbody / label
                run([compiler, '--compile', '--no-strip', '--cpu', cpu, nbody / (program + '.plenty'), '-o', binary], env=env)
                programs['nbody'][label] = binary
    for cpu in ['x86-64', 'x86-64-v3']:
        for shape in ['staged', 'fused']:
            label = cpu + '-c-' + shape
            binary = nbody / label
            run(['gcc', '-O2', '-march=' + cpu, '-ffp-contract=off', '-DKERNEL=' + shape,
                 OLD / 'driver.c', OLD / 'kernels.c', '-lm', '-o', binary])
            programs['nbody'][label] = binary
    for group, builds in programs.items():
        for steps in sorted(set([-1, 0, 1, 2, 3, 1000, args.steps])):
            expected = None
            for label, binary in builds.items():
                arguments = [binary, steps] + (['state'] if group == 'nbody' else [])
                values = run(arguments, capture_output=True, text=True).stdout.split()
                actual = [float(value).hex() for value in values]
                assert len(actual) == (37 if group == 'nbody' else 3), (group, label, values)
                if expected is None:
                    expected = actual
                assert actual == expected, (group, label, steps, actual, expected)
                verification.append([group, label, steps])
    data = dict(revision=run(['git', '-C', ROOT, 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip(),
                steps=args.steps, core=args.core, verification=verification, loops=ledger,
                instructions={}, cycles={}, wall_seconds={})
    for event, enabled, rounds, key in [('instructions:u', args.count, 3, 'instructions'),
                                         ('cycles:u', args.cycles, 9, 'cycles')]:
        if not enabled:
            continue
        for group, builds in programs.items():
            for repeat in range(rounds):
                labels = list(builds)
                labels = labels[repeat % len(labels):] + labels[:repeat % len(labels)]
                for label in labels:
                    value, elapsed = count(builds[label], args.steps, args.core, event)
                    data[key].setdefault(group + '-' + label, []).append(value)
                    if event == 'cycles:u':
                        data['wall_seconds'].setdefault(group + '-' + label, []).append(elapsed)
    data['medians'] = {key: {label: statistics.median(values) for label, values in data[key].items()}
                       for key in ['instructions', 'cycles', 'wall_seconds']}
    (work / 'native-results.json').write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps(dict(loops=ledger, medians=data['medians'], checked_cases=len(verification)), indent=2))


if __name__ == '__main__':
    main()
