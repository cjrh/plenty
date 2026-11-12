#!/usr/bin/env python3
"""Generate diagnostic five-body specializations, not replacement benchmarks.

Expands only the fixed-trip loops within advance; retains the time-step loop,
operation order, source checks, allocations, and original energy calculation.
"""
import argparse
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]


def expand(lines, bindings=None):
    bindings = {} if bindings is None else bindings
    out = []
    pos = 0
    while pos < len(lines):
        line = lines[pos]
        match = re.fullmatch(r'( *)for (i|j|p) in range\((n|pairs|i \+ 1, n)\):', line)
        if match:
            indent, var, bounds = match.groups()
            end = pos + 1
            while end < len(lines) and (not lines[end].strip() or len(lines[end]) - len(lines[end].lstrip()) > len(indent)):
                end += 1
            values = range(bindings['i'] + 1, 5) if var == 'j' else range(10 if var == 'p' else 5)
            body = [item[4:] if item.strip() else item for item in lines[pos+1:end]]
            for value in values:
                out.extend(expand(body, {**bindings, var: value}))
            pos = end
        else:
            for var, value in bindings.items():
                line = re.sub(r'\b' + var + r'\b', str(value), line)
            if bindings:
                suffix = '_'.join(f'{key}{value}' for key, value in bindings.items())
                for local in ('d2', 'mi', 'mj'):
                    line = re.sub(r'\b' + local + r'\b', local + '_' + suffix, line)
            out.append(line)
            pos += 1
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for filename, label in [('nbody.plenty', 'checked')]:
        source = (ROOT / 'benchmarks/game/n-body/plenty' / filename).read_text()
        start = source.index('def advance(')
        end = source.index('\n\ntype AppError', start)
        body = source[start:end]
        fixed = body.replace('n = len(bodies)', 'n = 5').replace('n * (n - 1) // 2', '10')
        (args.output / f'fixed-{label}.plenty').write_text(source[:start] + fixed + source[end:])
        body = '\n'.join(expand(fixed.splitlines())) + '\n'
        (args.output / f'unrolled-{label}.plenty').write_text(source[:start] + body + source[end:])


if __name__ == '__main__':
    main()
