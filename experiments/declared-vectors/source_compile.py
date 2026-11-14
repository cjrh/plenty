#!/usr/bin/env python3
"""Compare uncached AOT compiler instructions on unchanged source programs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', required=True, type=Path)
    parser.add_argument('--baseline-sha256', required=True)
    parser.add_argument('--candidate', type=Path, default=ROOT / 'target/release/plenty')
    parser.add_argument('--work', type=Path, default=ROOT / 'target/vec3-source/compile')
    parser.add_argument('--core', default='3')
    args = parser.parse_args()
    assert digest(args.baseline) == args.baseline_sha256, 'baseline identity mismatch'
    args.work.mkdir(parents=True, exist_ok=True)
    compilers = {'baseline': args.baseline.resolve(), 'candidate': args.candidate.resolve()}
    sources = {name: ROOT / f'benchmarks/game/{folder}/plenty/{stem}.plenty'
               for name, folder, stem in [('nbody', 'n-body', 'nbody'),
                                          ('fasta', 'fasta', 'fasta'),
                                          ('knucleotide', 'k-nucleotide', 'knucleotide')]}
    data = {'compilers': {name: {'path': str(p), 'sha256': digest(p)} for name, p in compilers.items()},
            'sources': {name: {'path': str(p.relative_to(ROOT)), 'sha256': digest(p)} for name, p in sources.items()},
            'core': args.core, 'cache': 'PLENTY_CACHE=off; --no-cache', 'samples': {}}
    for name, source in sources.items():
        data['samples'][name] = {kind: [] for kind in compilers}
        for repeat in range(3):
            for kind in (list(compilers) if repeat % 2 == 0 else list(reversed(compilers))):
                counter = args.work / f'{name}-{kind}-{repeat}.perf'
                command = ['perf', 'stat', '-x;', '-e', 'instructions:u', '-o', str(counter),
                           'taskset', '-c', args.core, str(compilers[kind]), '--compile',
                           '--cpu', 'x86-64-v3', '--no-cache', str(source), '-o',
                           str((args.work / f'{name}-{kind}').resolve())]
                subprocess.run(command, check=True, env={**os.environ, 'PLENTY_CACHE': 'off'},
                               stdout=subprocess.DEVNULL)
                rows = [line.split(';') for line in counter.read_text().splitlines()]
                counts = [float(row[0]) for row in rows if len(row) > 2 and row[2] == 'instructions:u']
                assert len(counts) == 1, counter
                data['samples'][name][kind].append(counts[0])
    data['medians'] = {name: {kind: statistics.median(rows) for kind, rows in samples.items()}
                       for name, samples in data['samples'].items()}
    data['driver_sha256'] = digest(Path(__file__))
    (args.work / 'results.json').write_text(json.dumps(data, indent=2) + '\n')
    print(json.dumps(data['medians'], indent=2))


if __name__ == '__main__':
    main()
