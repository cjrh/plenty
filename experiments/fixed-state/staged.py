#!/usr/bin/env python3
"""Expose scalar state while retaining n-body's difference/magnitude/update stages."""
import argparse
import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('output', type=Path)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location('structural', ROOT / 'experiments/nbody-structural/generate.py')
    structural = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(structural)
    structural.plenty(args.output)
    source = (args.output / 'locals.plenty').read_text()
    start = source.index('    for _ in range(steps):', source.index('def advance('))
    end = source.index('    bodies[0].x =', start)
    lines = [f'    mut {field}{k} = 0.0' for k in range(10) for field in ('dx', 'dy', 'dz', 'mag')]
    lines.append('    for _ in range(steps):')
    for k, (i, j) in enumerate(structural.PAIRS):
        for field in 'xyz':
            lines.append(f'        d{field}{k} = {field}{i} - {field}{j}')
    for k in range(10):
        lines += [f'        d2 = dx{k} * dx{k} + dy{k} * dy{k} + dz{k} * dz{k}',
                  f'        mag{k} = dt / (d2 * d2.sqrt())']
    for k, (i, j) in enumerate(structural.PAIRS):
        lines += [f'        mi = mass{i} * mag{k}', f'        mj = mass{j} * mag{k}']
        # Coordinates are independent; each velocity keeps its original pair order.
        for field in 'xyz':
            lines += [f'        v{field}{i} -= d{field}{k} * mj',
                      f'        v{field}{j} += d{field}{k} * mi']
    for i in range(5):
        for field in 'xyz':
            lines.append(f'        {field}{i} += dt * v{field}{i}')
    (args.output / 'staged.plenty').write_text(source[:start] + '\n'.join(lines) + '\n' + source[end:])


if __name__ == '__main__':
    main()
