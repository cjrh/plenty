#!/usr/bin/env python3
"""Interleaved `perf stat` rounds on one pinned core.

  measure.py [--core N] [--rounds 3] [--repeat 5] [--arg STEPS] LABEL=BINARY...

Each round runs every binary once under `perf stat -r REPEAT`, in rotated
order, and records user instructions, user cycles and task-clock. Prints the
per-round values and the median, with the load average at the start of each
round. `--json FILE` keeps every observation.
"""
import argparse
import json
import os
import statistics
import subprocess
import tempfile


def perf(core, repeat, binary, argv):
    with tempfile.NamedTemporaryFile('r', suffix='.perf', dir=os.environ.get('TMPDIR')) as out:
        subprocess.run(['taskset', '-c', str(core), 'perf', 'stat', '-r', str(repeat), '-x', ';',
                        '-e', 'instructions:u,cycles:u,task-clock:u', '-o', out.name, binary, *argv],
                       check=True, stdout=subprocess.DEVNULL)
        values = {}
        for line in out.read().splitlines():
            parts = line.split(';')
            if len(parts) > 2 and parts[0] and not line.startswith('#'):
                values[parts[2].split(':')[0]] = float(parts[0])
        return values


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--core', type=int, default=2)
    p.add_argument('--rounds', type=int, default=3)
    p.add_argument('--repeat', type=int, default=5)
    p.add_argument('--arg', action='append', default=[])
    p.add_argument('--per', type=float, default=1.0, help='divide counts by this (e.g. steps)')
    p.add_argument('--json')
    p.add_argument('bins', nargs='+')
    args = p.parse_args()
    bins = [b.split('=', 1) if '=' in b else (os.path.basename(b), b) for b in args.bins]
    bins = [(label, os.path.abspath(path)) for label, path in bins]
    obs = {label: [] for label, _ in bins}
    loads = []
    for r in range(args.rounds):
        loads.append(os.getloadavg()[0])
        order = bins[r % len(bins):] + bins[:r % len(bins)]
        for label, binary in order:
            obs[label].append(perf(args.core, args.repeat, binary, args.arg))
    width = max(len(label) for label, _ in bins)
    print(f'core {args.core}, {args.rounds} rounds x perf stat -r {args.repeat}, args {args.arg}, '
          f'load average at round starts {", ".join(f"{x:.2f}" for x in loads)}')
    print(f'{"build":<{width}}  {"instructions":>13}  {"cycles":>11}  {"cycles by round":<32}  {"ms":>8}')
    for label, _ in bins:
        ins = statistics.median(o['instructions'] for o in obs[label]) / args.per
        cyc = [o['cycles'] / args.per for o in obs[label]]
        ms = statistics.median(o['task-clock'] for o in obs[label])
        fmt = (lambda v: f'{v:,.1f}') if args.per != 1 else (lambda v: f'{v / 1e6:,.1f}M')
        print(f'{label:<{width}}  {fmt(ins):>13}  {fmt(statistics.median(cyc)):>11}  '
              f'{" ".join(fmt(c) for c in cyc):<32}  {ms:8.1f}')
    if args.json:
        with open(args.json, 'w') as f:
            json.dump({'core': args.core, 'repeat': args.repeat, 'args': args.arg, 'loads': loads,
                       'observations': obs}, f, indent=1)


if __name__ == '__main__':
    main()
