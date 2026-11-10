#!/usr/bin/env python3
"""Differential complete-state checks for the automatic optimizer."""
import argparse
import json
import os
from pathlib import Path
import random
import subprocess

ROOT = Path(__file__).resolve().parents[2]
ENV = dict(os.environ, PLENTY_CACHE='off')


def run(args):
    return subprocess.run([str(x) for x in args], capture_output=True, env=ENV)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--before', type=Path, required=True)
    p.add_argument('--after', type=Path, default=ROOT / 'target/release/plenty')
    p.add_argument('--work', type=Path, default=ROOT / 'target/fixed-state/random')
    args = p.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    generated = work / 'generated'
    generated.mkdir(exist_ok=True)
    subprocess.run(['python3', '-B', ROOT / 'experiments/nbody-structural/generate.py', generated], check=True)
    original = (generated / 'original.plenty').read_text()
    start = original.index('def starting_state(')
    end = original.index('\n\ndef ', start + 1)
    rng = random.Random(930)
    checked = 0
    for sample in range(16):
        rows = []
        for i in range(5):
            values = [2*i+rng.uniform(-.5,.5), rng.uniform(-2,2), rng.uniform(-2,2),
                      rng.uniform(-.05,.05), rng.uniform(-.05,.05), rng.uniform(-.05,.05), rng.uniform(.1,2)]
            rows.append('Body(' + ', '.join(repr(x) for x in values) + ')')
        state = 'def starting_state() -> Result[list[Body], AllocError]:\n    [' + ', '.join(rows) + ']\n'
        # A list literal returns its fallible Result directly.
        source = work / f'random-{sample}.plenty'
        source.write_text(original[:start] + state + original[end:])
        binaries = []
        for label, compiler in [('before', args.before.resolve()), ('after', args.after.resolve())]:
            output = work / f'{label}-{sample}'
            result = run([compiler, '--compile', source, '-o', output])
            assert result.returncode == 0, result.stderr.decode()
            binaries.append(output)
        for steps in [-1,0,1,3,17]:
            before, after = [run([binary, steps, 'state']) for binary in binaries]
            assert before.returncode == after.returncode == 0
            assert len(before.stdout.split()) == 37
            assert before.stdout == after.stdout, (sample,steps)
            checked += 1
    (work / 'verification.json').write_text(json.dumps({'seed':930,'random_initial_states':16,
        'steps':[-1,0,1,3,17], 'exact_full_state_cases':checked}, indent=2)+'\n')
    print(f'{checked} randomized full-state cases passed')


if __name__ == '__main__':
    main()
