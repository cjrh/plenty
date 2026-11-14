#!/usr/bin/env python3
"""Vary native function placement without changing its instruction bytes.

Eight offsets cover 8-byte phases of a 64-byte boundary. The current Plenty
reference stays unchanged, so this checks candidate-placement sensitivity,
not every possible placement of both compilers or a universal speed guarantee.
"""
import argparse
import importlib.util
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
spec = importlib.util.spec_from_file_location('paired_vectors', HERE / 'paired.py')
paired = importlib.util.module_from_spec(spec)
spec.loader.exec_module(paired)
run = paired.run


def prepare(source, work, steps):
    original = json.loads((source / 'preparation.json').read_text())
    if not original['complete']:
        raise RuntimeError('complete matched preparation first')
    reference = original['builds']['x86-64-v3-plenty-original']
    if paired.digest(Path(reference['path'])) != reference['sha256']:
        raise RuntimeError('current Plenty reference changed')
    builds = {'current-plenty-v3': reference}
    flags = ['-O2', '-march=x86-64', '-ffp-contract=off', '-fno-tree-vectorize',
             '-fno-tree-loop-distribute-patterns', '-frounding-math']
    checks = []
    for shape, index in [('wide', 4), ('wide-pairs', 5)]:
        raw = source / 'raw-avx2' / (str(index) + '-0.bin')
        if paired.digest(raw) != original['builds']['avx2-' + shape]['raw_sha256']:
            raise RuntimeError(shape + ': raw kernel changed')
        for pad in range(0, 64, 8):
            label = shape + '-pad-' + str(pad)
            assembly = work / (label + '.s')
            escaped = str(raw).replace('\\', '\\\\').replace('"', '\\"')
            assembly.write_text('.text\n.p2align 6\n' + (f'.skip {pad}, 0x90\n' if pad else '') +
                                '.globl advance\n.type advance,@function\nadvance:\n' +
                                f'.incbin "{escaped}"\n.size advance,.-advance\n' +
                                '.section .note.GNU-stack,"",@progbits\n')
            binary = work / label
            check = work / (label + '-verify')
            run(['gcc', *flags, '-DKERNEL=advance', paired.OLD / 'driver.c', assembly,
                 '-lm', '-o', binary])
            run(['gcc', *flags, HERE / 'verify-nbody-paired.c', assembly,
                 source / 'reference.o', '-lm', '-o', check])
            result = run([check], capture_output=True, text=True)
            (work / (label + '-verification.log')).write_text(result.stdout + result.stderr)
            symbols = run(['nm', '-n', binary], capture_output=True, text=True).stdout
            address = int(next(line.split()[0] for line in symbols.splitlines()
                               if line.split()[-1] == 'advance'), 16)
            if address % 64 != pad:
                raise RuntimeError(label + ': linker did not preserve requested phase')
            builds[label] = {'path': str(binary), 'sha256': paired.digest(binary),
                             'raw_sha256': paired.digest(raw), 'entry_address': address,
                             'entry_phase_mod64': address % 64}
            checks.append({'name': label, 'result': result.stdout.strip()})
    preparation = {'revision': original['revision'], 'source_preparation_sha256':
                   paired.digest(source / 'preparation.json'), 'builds': builds,
                   'native_checks': checks, 'output_checks': paired.compare_outputs(builds, steps),
                   'complete': True, 'source_sha256': paired.digest(Path(__file__))}
    (work / 'preparation.json').write_text(json.dumps(preparation, indent=2) + '\n')
    print('verified', len(builds), 'programs across eight native placements')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=ROOT / 'target/declared-vectors-paired')
    parser.add_argument('--work', type=Path, default=ROOT / 'target/declared-vectors-placement')
    parser.add_argument('--steps', type=int, default=1_000_000)
    parser.add_argument('--core', type=int, default=3)
    parser.add_argument('--measure-only', action='store_true')
    args = parser.parse_args()
    if args.steps <= 0 or args.core < 0:
        parser.error('steps must be positive and core nonnegative')
    source, work = args.source.resolve(), args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    if args.measure_only:
        preparation = json.loads((work / 'preparation.json').read_text())
        if args.steps not in {check['steps'] for check in preparation['output_checks']}:
            parser.error('prepare and verify this step count before measuring')
        paired.measure(work, preparation, args.steps, args.core)
    else:
        prepare(source, work, args.steps)


if __name__ == '__main__':
    main()
