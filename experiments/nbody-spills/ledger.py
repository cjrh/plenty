#!/usr/bin/env python3
"""Classify one straight-line loop body and find avoidable data movement.

Reads a `plenty --dump asm` listing or `objdump -d --no-show-raw-insn` output
on stdin, takes the longest innermost loop of FUNCTION (default: the longest
loop in the file), and prints:

- a category ledger (arithmetic, stack loads, stack stores, register copies...),
- stack loads that a memory operand could replace: the loaded register's only
  read is as the source of the next scalar operation that reads it,
- loads followed by a copy of the loaded register, which is then dead (the
  load could target the copy's destination),
- stores that write a slot's own unchanged value back.

Works for AT&T syntax with %rsp- or %rbp-based slots.
"""
import argparse
import importlib.util
import json
import re
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('kernel_loops', ROOT / 'experiments/kernels/loops.py')
loops = importlib.util.module_from_spec(spec)
spec.loader.exec_module(loops)

ARITH = re.compile(r'v?(add|sub|mul|div|sqrt|min|max)(sd|ss|pd|ps)$')
MOVES = re.compile(r'v?(movdqu|movdqa|movsd|movss|movaps|movapd|movups|movupd|movq|movd|mov)$')
SLOT = re.compile(r'(-?(?:0x)?[0-9a-fA-F]*)\(%r(?:sp|bp)\)$')
LANES = ('unpck', 'shuf', 'pshuf', 'perm', 'blend', 'extract', 'insert', 'movlh', 'movhl', 'movddup', 'punpck')


def split_ops(text):
    out, depth, cur = [], 0, ''
    for ch in text:
        if ch == '(':
            depth += 1
        elif ch == ')':
            depth -= 1
        if ch == ',' and depth == 0:
            out.append(cur.strip())
            cur = ''
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def is_slot(op):
    return bool(SLOT.search(op))


def is_mem(op):
    return '(' in op


def classify(m, ops):
    if ARITH.match(m):
        return 'FP arithmetic, stack operand' if any(is_slot(o) for o in ops) else (
            'FP arithmetic, memory operand' if any(is_mem(o) for o in ops) else 'FP arithmetic')
    if MOVES.match(m) and len(ops) == 2:
        src, dst = ops
        if is_slot(src):
            return 'stack loads'
        if is_slot(dst):
            return 'stack stores'
        if is_mem(src):
            return 'other loads'
        if is_mem(dst):
            return 'other stores'
        if src.startswith('$'):
            return 'constants'
        return 'register copies'
    if m.lstrip('v').startswith(LANES):
        return 'lane rearrangements'
    if m.lstrip('v').startswith(('xor', 'pxor')):
        return 'zeroing'
    if m.startswith('call'):
        return 'calls'
    if m.startswith(('j', 'cmp', 'test', 'dec', 'inc', 'sub', 'add', 'lea')):
        return 'integer/control'
    return 'other: ' + m


def reads_writes(m, ops):
    """Registers read and written by one instruction (xmm and gpr names)."""
    regs = lambda o: re.findall(r'%\w+', o)
    if not ops:
        return set(), set()
    if MOVES.match(m) and len(ops) == 2:
        src, dst = ops
        r = set(regs(src))
        if is_mem(dst):
            return r | set(regs(dst)), set()
        return r, {dst}
    if m.lstrip('v').startswith('sqrt') and len(ops) == 2:
        return set(regs(ops[0])), {ops[1]}
    if m.startswith(('cmp', 'test', 'j', 'ucomis', 'comis')):
        return set(sum((regs(o) for o in ops), [])), set()
    dst = ops[-1]
    r = set(sum((regs(o) for o in ops), []))
    if is_mem(dst):
        return r, set()
    if m.lstrip('v').startswith(('xor', 'pxor')) and len(ops) == 2 and ops[0] == ops[1]:
        return set(), {dst}
    return r, {dst}


def analyse(body):
    n = len(body)
    insts = [(m, split_ops(op)) for _, m, op in body]
    cats = Counter(classify(m, ops) for m, ops in insts)
    rw = [reads_writes(m, ops) for m, ops in insts]

    def next_refs(i, reg):
        """Indices (cyclic) that read `reg` after i, until it is overwritten."""
        out = []
        for step in range(1, n + 1):
            j = (i + step) % n
            r, w = rw[j]
            if reg in r:
                out.append(j)
            if reg in w:
                break
        return out

    foldable = copy_after_load = 0
    fold_examples = []
    for i, (m, ops) in enumerate(insts):
        if classify(m, ops) != 'stack loads':
            continue
        reg = ops[1]
        refs = next_refs(i, reg)
        if len(refs) != 1:
            continue
        j = refs[0]
        mj, oj = insts[j]
        # slot must not be written between the load and the use
        slot = ops[0]
        clobbered = any(classify(*insts[k % n]) == 'stack stores' and insts[k % n][1][1] == slot
                        for k in range(i + 1, j if j > i else j + n))
        if clobbered:
            continue
        if ARITH.match(mj) and len(oj) == 2 and oj[0] == reg and oj[1] != reg:
            foldable += 1
            if len(fold_examples) < 3:
                fold_examples.append((body[i][0], body[j][0]))
        elif classify(mj, oj) == 'register copies' and oj[0] == reg:
            copy_after_load += 1

    # Value numbering along the straight line for redundant stores.
    val = {}
    fresh = iter(range(1, 1 << 30))
    redundant = 0
    for m, ops in insts:
        c = classify(m, ops)
        if c == 'stack loads':
            src, dst = ops
            val.setdefault(('s', src), next(fresh))
            val[('r', dst)] = val[('s', src)]
        elif c == 'stack stores':
            src, dst = ops
            v = val.setdefault(('r', src), next(fresh))
            if val.get(('s', dst)) == v:
                redundant += 1
            val[('s', dst)] = v
        elif c == 'register copies':
            src, dst = ops
            val[('r', dst)] = val.setdefault(('r', src), next(fresh))
        else:
            for w in reads_writes(m, ops)[1]:
                val[('r', w)] = next(fresh)
    return {
        'total': n,
        'categories': dict(sorted(cats.items(), key=lambda kv: -kv[1])),
        'loads a memory operand could replace': foldable,
        'loads whose only use is a register copy': copy_after_load,
        'stores of a slot\'s unchanged value': redundant,
        'examples': [f'{a:#x}->{b:#x}' for a, b in fold_examples],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--function')
    parser.add_argument('--all', action='store_true', help='report every loop')
    args = parser.parse_args()
    found = [(name, start, end, body) for name, start, end, body in loops.loops(loops.parse(sys.stdin))
             if not args.function or name == args.function]
    if not found:
        sys.exit('no loop found')
    if not args.all:
        found = [max(found, key=lambda t: len(t[3]))]
    out = {}
    for name, start, end, body in found:
        out[f'{name} {start:#x}-{end:#x}'] = analyse(body)
    print(json.dumps(out, indent=2))


if __name__ == '__main__':
    main()
