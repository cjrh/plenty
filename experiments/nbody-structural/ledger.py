#!/usr/bin/env python3
"""Account for every instruction in the single-step unrolled hot loops.

Categories are exclusive. Arithmetic with a stack operand is arithmetic, not
an extra move. This is an instruction ledger, not additive causal speedups.
"""
import argparse
from collections import Counter
import importlib.util
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('kernel_loops', ROOT / 'experiments/kernels/loops.py')
loops = importlib.util.module_from_spec(spec)
spec.loader.exec_module(loops)


def category(mnemonic, operand):
    m = mnemonic.removeprefix('v')
    if re.fullmatch(r'(add|sub|mul|div|sqrt)(sd|ss)', m):
        return 'scalar FP arithmetic'
    if re.fullmatch(r'(add|sub|mul|div|sqrt)(pd|ps)', m):
        return 'packed FP arithmetic'
    if m.startswith('mov'):
        if re.search(r'%r(?:sp|bp)', operand):
            return 'stack moves'
        if '(' in operand:
            return 'other memory moves'
        return 'register copies'
    if m.startswith(('unpck', 'shuf', 'pshuf', 'perm', 'blend', 'extract', 'insert')):
        return 'lane rearrangements'
    if m.startswith(('xor', 'pxor')):
        return 'zeroing'
    if m.startswith(('call',)):
        return 'calls (callees excluded)'
    if m.startswith(('j', 'cmp', 'test', 'dec', 'inc', 'sub', 'add', 'lea')):
        return 'integer/control'
    return 'other: ' + mnemonic


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('work', type=Path)
    args = parser.parse_args()
    result = {}
    sources = [('plenty-locals', args.work / 'locals-dump/asm.txt', 'advance'),
               ('clang-locals', args.work / 'clang-locals.asm', 'locals'),
               ('clang-scalar-locals', args.work / 'clang-scalar-locals.asm', 'locals')]
    for isa in ('baseline', 'avx'):
        sources.append((f'clif-{isa}', args.work / f'ir-{isa}.asm', None))
    for label, path, function in sources:
        with path.open() as f:
            bodies = list(loops.loops(loops.parse(f)))
        for name, start, end, body in bodies:
            if function and name != function:
                continue
            key = label if function else label + '-' + name.removeprefix('%advance_')
            counts = Counter(category(m, op) for _, m, op in body)
            assert sum(counts.values()) == len(body)
            result[key] = {'start': hex(start), 'end': hex(end), 'total': len(body), 'categories': dict(counts)}
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
