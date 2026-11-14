#!/usr/bin/env python3
"""Measure separately built compiler and compile_bench pairs with caches disabled."""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path, help='baseline/candidate-plenty and -compile-bench')
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--cpu', default='3')
    parser.add_argument('--repeats', type=int, default=3)
    args = parser.parse_args()
    if args.repeats < 1:
        parser.error('--repeats must be positive')
    root = args.directory.resolve()
    cases = [('aot-1000x10', None),
             ('spectral', 'benchmarks/game/spectral-norm/plenty/spectralnorm.plenty'),
             ('bounded', 'experiments/kernels/bounded-arguments/spectral_norm.plenty'),
             ('fasta', 'benchmarks/game/fasta/plenty/fasta.plenty')]
    report = {'cpu': args.cpu, 'repeats': args.repeats, 'samples': []}
    env = os.environ | {'PLENTY_CACHE': 'off'}
    for iteration in range(args.repeats):
        for name, source in cases:
            for variant in (['baseline', 'candidate'] if iteration % 2 == 0
                            else ['candidate', 'baseline']):
                if source is None:
                    command = [str(root/(variant+'-compile-bench')), '1000', '10', '--workload', 'aot']
                else:
                    command = [str(root/(variant+'-plenty')), '--no-cache', '--compile', source,
                               '-o', str(root/'perf-compile-output')]
                command = ['perf', 'stat', '-x', ';', '-e', 'instructions:u,task-clock:u',
                           'taskset', '-c', args.cpu, *command]
                run = subprocess.run(command, capture_output=True, env=env, check=True)
                stderr = run.stderr.decode()
                counts = {}
                for line in stderr.splitlines():
                    fields = line.split(';')
                    if len(fields) >= 3 and fields[2] in ('instructions:u', 'task-clock:u'):
                        counts[fields[2]] = float(fields[0])
                if len(counts) != 2:
                    raise RuntimeError('missing perf counter: '+stderr)
                report['samples'].append(dict(name=name, variant=variant, round=iteration,
                                               command=command, stdout=run.stdout.decode(),
                                               stderr=stderr, **counts))
                args.output.write_text(json.dumps(report, indent=2)+'\n')
                print(name, variant, iteration, flush=True)
    for name, _ in cases:
        medians = {variant: {metric: statistics.median(
            s[metric] for s in report['samples'] if s['name']==name and s['variant']==variant)
            for metric in ('instructions:u', 'task-clock:u')}
            for variant in ('baseline', 'candidate')}
        print(name, json.dumps(medians), flush=True)


if __name__ == '__main__':
    main()
