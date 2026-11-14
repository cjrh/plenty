#!/usr/bin/env python3
"""Build/check/count matched declared-vector source probes; no compiler changes."""
import argparse
from collections import Counter
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('loops', ROOT / 'experiments/kernels/loops.py')
loops = importlib.util.module_from_spec(spec)
spec.loader.exec_module(loops)


def run(args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def category(mnemonic, operand):
    mnemonic = mnemonic.removeprefix('v')
    if re.fullmatch(r'(add|sub|mul|div|sqrt)(sd|ss)', mnemonic):
        return 'scalar FP arithmetic'
    if re.fullmatch(r'(add|sub|mul|div|sqrt)(pd|ps)', mnemonic):
        return 'packed FP arithmetic'
    if mnemonic.startswith('mov'):
        if re.search(r'\b(?:rsp|rbp)\b|%r(?:sp|bp)', operand):
            return 'stack moves'
        if '[' in operand or '(' in operand:
            return 'other memory moves'
        return 'register copies'
    if mnemonic.startswith(('unpck', 'shuf', 'pshuf', 'perm', 'blend', 'extract', 'insert')):
        return 'lane rearrangements'
    if mnemonic.startswith(('xor', 'pxor')):
        return 'zeroing'
    if mnemonic.startswith('call'):
        return 'calls (callee instructions excluded)'
    if mnemonic.startswith(('j', 'cmp', 'test', 'dec', 'inc', 'sub', 'add', 'lea')):
        return 'integer/control'
    return 'other: ' + mnemonic


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work', type=Path, default=ROOT / 'target/declared-vectors')
    parser.add_argument('--steps', type=int, default=1_000_000)
    parser.add_argument('--count', action='store_true', help='perf instructions:u, pinned to --core')
    parser.add_argument('--compile-count', action='store_true', help='count three uncached release compilations per Plenty source/CPU')
    parser.add_argument('--core', type=int, default=0)
    args = parser.parse_args()
    if args.steps <= 0:
        parser.error('--steps must be positive (zero and negative cases are checked separately)')
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PLENTY_CACHE='off')
    compiler = ROOT / 'target/release/plenty'
    binaries, ledger, objects = {}, {}, {}
    for cpu, c_cpu in [('x86-64', 'x86-64'), ('x86-64-v3', 'x86-64-v3')]:
        for shape in ['scalar', 'ordinary-class', 'array', 'wrapped-array', 'simd']:
            label = cpu + '-' + shape
            output = work / label
            run([compiler, '--compile', '--no-strip', '--cpu', cpu, HERE / (shape + '.plenty'), '-o', output], env=env)
            dump = work / (label + '-dump')
            run([compiler, '--dump', 'ops,clif-opt,asm', '--cpu', cpu, HERE / (shape + '.plenty'), '-o', dump], env=env)
            binaries[label] = output
            objects[label] = dump / 'asm.txt'
        label = cpu + '-c'
        flags = ['-O2', '-march=' + c_cpu, '-ffp-contract=off', '-fno-tree-loop-distribute-patterns']
        obj = work / (label + '.o')
        run(['gcc', *flags, '-c', HERE / 'kernels.c', '-o', obj])
        run(['gcc', *flags, HERE / 'driver.c', obj, '-o', work / label])
        binaries[label] = work / label
        with (work / (label + '.asm')).open('w') as file:
            run(['objdump', '-d', '--no-show-raw-insn', obj], stdout=file)
        objects[label] = work / (label + '.asm')
    checks = []
    for steps in sorted(set([-1, 0, 1, 2, 3, 1000, args.steps])):
        expected = None
        for label, binary in binaries.items():
            actual = run([binary, steps], capture_output=True, text=True).stdout.split()
            actual = [float(value).hex() for value in actual]
            assert len(actual) == 3, (label, actual)
            if expected is None:
                expected = actual
            assert actual == expected, (label, steps, actual, expected)
            checks.append([label, steps, actual])
    for label, source in objects.items():
        with source.open() as file:
            bodies = list(loops.loops(loops.parse(file)))
        ledger[label] = []
        for name, start, end, body in bodies:
            counts = Counter(category(m, op) for _, m, op in body)
            ledger[label].append(dict(function=name, start=hex(start), end=hex(end),
                                      static_loop_union=len(body), categories=dict(counts)))
    data = dict(revision=run(['git', '-C', ROOT, 'rev-parse', 'HEAD'], capture_output=True, text=True).stdout.strip(),
                compiler_version=run([compiler, '--version'], capture_output=True, text=True).stdout.strip(),
                gcc_version=run(['gcc', '--version'], capture_output=True, text=True).stdout.splitlines()[0],
                host=platform.platform(), core=args.core, steps=args.steps,
                verification=checks, loops=ledger, instruction_counts={}, zero_instruction_counts={}, compile_instruction_counts={})
    if args.count:
        for repeat in range(3):
            # Rotate each round's order to reduce drift in startup/cache conditions.
            names = list(binaries)
            names = names[repeat:] + names[:repeat]
            for label in names:
                output = run(['perf', 'stat', '-x,', '-e', 'instructions:u', '--',
                              'taskset', '-c', args.core, binaries[label], args.steps],
                             capture_output=True, text=True).stderr
                line = next(line for line in output.splitlines() if ',instructions:u,' in line)
                count = int(line.split(',')[0])
                data['instruction_counts'].setdefault(label, []).append(count)
        data['instruction_medians'] = {label: statistics.median(values)
                                       for label, values in data['instruction_counts'].items()}
        for label, binary in binaries.items():
            for _ in range(3):
                output = run(['perf', 'stat', '-x,', '-e', 'instructions:u', '--',
                              'taskset', '-c', args.core, binary, 0], capture_output=True, text=True).stderr
                line = next(line for line in output.splitlines() if ',instructions:u,' in line)
                data['zero_instruction_counts'].setdefault(label, []).append(int(line.split(',')[0]))
        data['incremental_instructions_per_step'] = {
            label: (statistics.median(data['instruction_counts'][label]) - statistics.median(values)) / args.steps
            for label, values in data['zero_instruction_counts'].items()}
    if args.compile_count:
        for repeat in range(3):
            for label in binaries:
                if label.endswith('-c'):
                    continue
                cpu = next(cpu for cpu in ['x86-64-v3', 'x86-64'] if label.startswith(cpu + '-'))
                shape = label[len(cpu) + 1:]
                output = run(['perf', 'stat', '-x,', '-e', 'instructions:u', '--',
                              'taskset', '-c', args.core, compiler, '--compile', '--cpu', cpu,
                              HERE / (shape + '.plenty'), '-o', work / 'compile-probe'],
                             env=env, capture_output=True, text=True).stderr
                line = next(line for line in output.splitlines() if ',instructions:u,' in line)
                data['compile_instruction_counts'].setdefault(label, []).append(int(line.split(',')[0]))

    (work / 'source-results.json').write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps({key: value for key, value in data.items() if key != 'verification'}, indent=2))


if __name__ == '__main__':
    main()
