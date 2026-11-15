#!/usr/bin/env python3
"""Generate shared cases, compare oracle output, then measure repeated batches."""
import argparse
import csv
import hashlib
import json
import math
import os
from pathlib import Path
import random
import statistics
import struct
import subprocess

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent


def run(command, **kwargs):
    return subprocess.run(command, check=True, text=True, capture_output=True, **kwargs)


def generate(out):
    rng = random.Random(481983)
    cases = [(0, 0, 0), (0, 1, 64), (1, -1, 65), (-1, 1, 4096)]
    for bits in (1, 31, 32, 53, 63, 64, 65, 127, 128, 1023, 1024):
        for delta in (-1, 0, 1):
            n = (1 << bits) + delta
            cases.extend(((n, -3, bits + 1), (-n, 3, bits)))
    for limbs in (2, 3, 31, 32, 33, 64, 128):
        bits = 64 * limbs
        for a in ((1 << bits) - 1, rng.getrandbits(bits) | (1 << (bits - 1))):
            b = rng.getrandbits(bits) | (1 << (bits - 1))
            for sa, sb in ((1, 1), (-1, 1), (1, -1), (-1, -1)):
                cases.append((sa * a, sb * b, 63))
        cases.append(((1 << bits) - 1, (1 << 65) + 1, 65))
        cases.append((17, (1 << bits) - 1, 0))
    # Construct normalized base-2^32 division estimates requiring correction.
    # For a < b*B, the exact single quotient digit is a//b. The leading-two
    # digit estimate ignores the low divisor digits and can exceed it.
    base = 1 << 32
    corrections = 0
    while corrections < 12:
        b = rng.randrange(base // 2, base) * base + rng.randrange(base)
        a = rng.randrange(b, b * base)
        estimate = min((a // base) // (b // base), base - 1)
        if estimate > a // b:
            cases.extend(((a, b, 32), (-a, b, 32), (a, -b, 32)))
            corrections += 1
    # Halfway cases: guard/sticky bits, both even and odd significands;
    # the overflow boundary includes values immediately around its midpoint.
    for n in ((1 << 54) + 2, (1 << 54) + 6,
              (1 << 100) + (1 << 46), (1 << 100) + (1 << 46) + 1,
              (1 << 1024) - (1 << 970) - 1,
              (1 << 1024) - (1 << 970), 1 << 1024):
        cases.extend(((n, 1, 0), (-n, -1, 0)))
    texts = [(f'{a:x}', f'{b:x}', shift) for a, b, shift in cases]
    texts.extend((a, '1', 0) for a in ('-000', '+000', '+0001', '-0001', 'DEADBEEF'))
    corpus = ''.join(f'{a}\t{b}\t{shift}\n' for a, b, shift in texts)
    (out / 'corpus.tsv').write_text(corpus)
    template = (HERE / 'oracle.plenty.in').read_text()
    groups = []
    names = []
    for offset in range(0, len(texts), 20):
        name = f'group_{offset}'
        names.append(f'{name}()?')
        calls = '\n    '.join(f'check("{a}", "{b}", {s})?'
                              for a, b, s in texts[offset:offset + 20])
        groups.append(f'def {name}() -> Result[(), AllocError | BigIntError | IoError | FormatError]:\n'
                      f'    {calls}\n    Ok(())\n\n')
    template = template.replace('def main()', ''.join(groups) + 'def main()')
    (out / 'oracle.plenty').write_text(template.replace('@CASES@', '\n    '.join(names)))
    benchmark = []
    for bits in (16, 256, 2048, 8192, 65536):
        a = rng.getrandbits(bits) | (1 << (bits - 1))
        b = rng.getrandbits(bits) | (1 << (bits - 1))
        benchmark.append({'bits': bits, 'a': f'{a:x}', 'b': f'{b:x}', 'product': f'{a*b:x}'})
    (out / 'benchmark-inputs.json').write_text(json.dumps(benchmark, indent=2) + '\n')
    (out / 'corpus-summary.json').write_text(json.dumps({
        'seed': 481983, 'cases': len(texts), 'quotient_estimate_correction_cases': corrections,
        'sha256': hashlib.sha256(corpus.encode()).hexdigest(),
    }, indent=2) + '\n')


def compare(out, malachite):
    for name, cmd in (
        ('malachite', [str(malachite), 'oracle', str(out / 'corpus.tsv')]),
        ('plenty', [str(out / 'oracle')]),
    ):
        result = run(cmd)
        (out / f'{name}-oracle.txt').write_text(result.stdout)
    left = (out / 'malachite-oracle.txt').read_text().splitlines()
    right = (out / 'plenty-oracle.txt').read_text().splitlines()
    assert len(left) == len(right), (len(left), len(right))
    for number, (a, b) in enumerate(zip(left, right), 1):
        if a.startswith('float ') and b.startswith('float '):
            # Normalize spelling only; compare exact IEEE bits, including sign.
            a = struct.pack('>d', float(a[6:]))
            b = struct.pack('>d', float(b[6:]))
        assert a == b, f'output line {number}: {a!r} != {b!r}'
    result = json.loads((out / 'corpus-summary.json').read_text())
    result['matching_output_lines'] = len(left)
    (out / 'differential-summary.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


def benchmark(out, malachite):
    cpu = min(os.sched_getaffinity(0))
    bins = {'plenty': out / 'bench', 'malachite': malachite}
    rows = []
    instructions = []
    for entry in json.loads((out / 'benchmark-inputs.json').read_text()):
        bits = entry['bits']
        for op in ('add', 'mul', 'div', 'parse', 'render'):
            a = entry['product'] if op == 'div' else entry['a']
            b = entry['b']
            args = [op, a, b]
            # Calibrate a shared count against Plenty, then use that same batch
            # for both engines. Setup and rendering inputs happen before timers.
            pilot = run(['taskset', '-c', str(cpu), str(bins['plenty']), *args, '1'])
            ns, _ = map(int, pilot.stdout.split())
            count = max(2, min(1_000_000, math.ceil(40_000_000 / max(ns, 1))))
            checksum = None
            for repeat in range(5):
                order = ('plenty', 'malachite') if repeat % 2 == 0 else ('malachite', 'plenty')
                for engine in order:
                    command = ['taskset', '-c', str(cpu), str(bins[engine]), *args, str(count)]
                    result = run(command)
                    ns, value = map(int, result.stdout.split())
                    checksum = value if checksum is None else checksum
                    assert value == checksum, (bits, op, engine, value, checksum)
                    rows.append([bits, op, engine, repeat, count, ns, value])
            # Count doubled and single batches. Their difference removes process
            # startup, input parsing/rendering and a single output operation.
            # Three pairs expose counter spread; raw perf output stays in out/.
            for engine in bins:
                for repeat in range(3):
                    counts = []
                    for factor in (1, 2):
                        tag = f'perf-{bits}-{op}-{engine}-{repeat}-{factor}'
                        cmd = ['perf', 'stat', '-x,', '-e', 'instructions:u', '--',
                               'taskset', '-c', str(cpu), str(bins[engine]),
                               *args, str(count * factor)]
                        result = run(cmd)
                        (out / f'{tag}.txt').write_text(result.stderr)
                        (out / f'{tag}.stdout').write_text(result.stdout)
                        event = next(row for row in csv.reader(result.stderr.splitlines())
                                     if len(row) > 2 and row[2] == 'instructions:u')
                        counts.append(int(event[0]))
                    instructions.append([bits, op, engine, repeat, count, *counts,
                                         (counts[1] - counts[0]) / count])
            print(f'completed {bits} bits {op}', flush=True)
    for name, header, data in (
        ('wall.csv', ['bits', 'operation', 'engine', 'repeat', 'count', 'ns', 'checksum'], rows),
        ('instructions.csv', ['bits', 'operation', 'engine', 'repeat', 'count',
                              'instructions_n', 'instructions_2n', 'instructions_per_op'], instructions),
    ):
        with (out / name).open('w') as stream:
            writer = csv.writer(stream)
            writer.writerow(header)
            writer.writerows(data)
    summary = ['| Bits | Operation | Instructions/op Plenty | Malachite | ns/op Plenty | Malachite |',
               '| ---: | --- | ---: | ---: | ---: | ---: |']
    for entry in json.loads((out / 'benchmark-inputs.json').read_text()):
        bits = entry['bits']
        for op in ('add', 'mul', 'div', 'parse', 'render'):
            ic = [statistics.median(r[-1] for r in instructions if r[:3] == [bits, op, e])
                  for e in bins]
            ns = [statistics.median(r[5] / r[4] for r in rows if r[:3] == [bits, op, e])
                  for e in bins]
            summary.append(f'| {bits} | {op} | {ic[0]:.1f} | {ic[1]:.1f} | {ns[0]:.1f} | {ns[1]:.1f} |')
    (out / 'summary.md').write_text('\n'.join(summary) + '\n')
    metadata = {key: run(cmd).stdout.strip() for key, cmd in (
        ('rustc', ['rustc', '--version', '--verbose']), ('host', ['uname', '-a']),
        ('cpu', ['lscpu']), ('commit', ['git', '-C', str(REPO), 'rev-parse', 'HEAD']),
    )}
    metadata.update(cpu_affinity=cpu, rust_profile='release,opt-level=3,codegen-units=1,panic=abort',
                    plenty_profile='release compiler, optimized default program, no runtime-checks',
                    malachite='0.14.0, default features, no production dependency')
    (out / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('mode', choices=('generate', 'compare', 'benchmark'))
    parser.add_argument('--out', type=Path, default=REPO / 'target/bigint-comparison')
    parser.add_argument('--malachite', type=Path)
    options = parser.parse_args()
    options.out.mkdir(parents=True, exist_ok=True)
    if options.mode == 'generate':
        generate(options.out)
    elif options.mode == 'compare':
        compare(options.out, options.malachite)
    else:
        benchmark(options.out, options.malachite)


if __name__ == '__main__':
    main()
