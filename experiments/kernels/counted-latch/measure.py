#!/usr/bin/env python3
"""Compare separately compiled benchmark executables; retain every perf sample."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path, help='NAME-baseline and NAME-candidate executables')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--cpu', default='3')
    parser.add_argument('--repeats', type=int, default=3)
    parser.add_argument('--only', action='append', default=[], help='benchmark-name prefix to sample')
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error('--repeats must be positive')
    directory = args.directory.resolve()
    repo = Path(__file__).resolve().parents[3]
    manifests = {p.parent.name: json.loads(p.read_text())
                 for p in (repo / 'benchmarks/game').glob('*/benchmark.json')}
    cases = []
    for p in sorted(directory.glob('*-baseline')):
        name = p.name.removesuffix('-baseline')
        if args.only and not any(name.startswith(prefix) for prefix in args.only):
            continue
        benchmark = 'spectral-norm' if name == 'bounded-spectral' else next(
            (key for key in manifests if name.startswith(key+'-')), None)
        if benchmark is None:
            continue
        manifest = manifests[benchmark]
        # Share deterministic input while retaining each manifest's quick prefix.
        stdin = manifest['input'] == 'fasta-stdin'
        command_args = [] if stdin else [str(manifest['sizes']['quick'])]
        if stdin:
            stdin = directory / (benchmark+'-input.txt')
            with stdin.open('wb') as output:
                subprocess.run([str(directory/'fasta-fasta-baseline'), str(manifest['sizes']['quick'])],
                               stdout=output, check=True)
        cases.append((name, command_args, stdin))
    if not cases:
        parser.error('no matching benchmark executables')
    report = {'host': platform.platform(), 'cpu': args.cpu, 'repeats': args.repeats,
              'cpuinfo': Path('/proc/cpuinfo').read_text(), 'samples': []}
    expected = {}
    for round_number in range(args.repeats):
        for name, command_args, stdin in cases:
            for variant in (['baseline', 'candidate'] if round_number % 2 == 0
                            else ['candidate', 'baseline']):
                command = ['perf', 'stat', '-x', ';', '-e', 'instructions:u,task-clock:u',
                           'taskset', '-c', args.cpu, str(directory/(name+'-'+variant)), *command_args]
                with (stdin or Path('/dev/null')).open('rb') as source:
                    run = subprocess.run(command, stdin=source, capture_output=True, check=True)
                digest = hashlib.sha256(run.stdout).hexdigest()
                prior = expected.setdefault(name, digest)
                if prior != digest:
                    raise RuntimeError(f'{name} {variant}: output changed {prior} -> {digest}')
                counts = {}
                stderr = run.stderr.decode()
                for line in stderr.splitlines():
                    fields = line.split(';')
                    if len(fields) >= 3 and fields[2] in ('instructions:u', 'task-clock:u'):
                        counts[fields[2]] = float(fields[0])
                if len(counts) != 2:
                    raise RuntimeError('missing perf counter: '+stderr)
                report['samples'].append(dict(name=name, variant=variant, round=round_number,
                                               command=command, output_sha256=digest,
                                               perf_csv=stderr, **counts))
                args.output.write_text(json.dumps(report, indent=2)+'\n')
            print(name, round_number, flush=True)
    for name, _, _ in cases:
        medians = {variant: {metric: statistics.median(
            s[metric] for s in report['samples'] if s['name']==name and s['variant']==variant)
            for metric in ('instructions:u', 'task-clock:u')}
            for variant in ('baseline', 'candidate')}
        print(name, json.dumps(medians), flush=True)


if __name__ == '__main__':
    main()
