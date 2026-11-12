#!/usr/bin/env python3
"""Rewrite a disassembled kernel with one class of data movement removed.

Input: the kernel harness listing of one function (`addr:  mnemonic operands`,
AT&T syntax). Output: GNU assembler source for the same function, optionally
with peepholes applied. Each peephole removes instructions only where a
whole-function scan proves the result unchanged; `measure-peepholes.sh` checks
the complete state against the unmodified kernel. This measures what a cause
costs in cycles before any compiler change; it is not a compiler pass.

  --fold       a stack load whose register is read once, as the source of a
               scalar operation, becomes that operation's memory operand
  --copies     a stack load whose register is read once, by a register copy,
               loads into the copy's destination
  --stores     drop a store of the value its slot already holds
  --scalar     8-byte movsd for stack loads and stores instead of movdqu
  --pad N      N bytes of nop before the function (loop placement)
"""
import argparse
import re
import sys

LINE = re.compile(r'^\s*([0-9a-f]+):\s+(\S+)\s*(.*)$')
ARITH = re.compile(r'(add|sub|mul|div|sqrt)(sd|ss)$')
SLOT = re.compile(r'^-?(?:0x)?[0-9a-f]*\(%rsp\)$')
REG = re.compile(r'%\w+')


def ops_of(text):
    out, depth, cur = [], 0, ''
    for ch in text:
        depth += (ch == '(') - (ch == ')')
        if ch == ',' and depth == 0:
            out.append(cur.strip())
            cur = ''
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


class Inst:
    def __init__(self, addr, m, ops):
        self.addr, self.m, self.ops, self.dead = addr, m, ops, False

    def kind(self):
        m, o = self.m, self.ops
        if m in ('movdqu', 'movsd', 'movdqa', 'movaps') and len(o) == 2:
            if SLOT.match(o[0]):
                return 'load'
            if SLOT.match(o[1]):
                return 'store'
            if '(' not in o[0] and '(' not in o[1] and not o[0].startswith('$'):
                return 'copy'
        return 'other'

    def reads_writes(self):
        m, o = self.m, self.ops
        allregs = set(REG.findall(' '.join(o)))
        if not o or m.startswith('j'):
            return allregs, set()
        dst = o[-1]
        if '(' in dst:
            return allregs, set()
        if m.startswith(('cmp', 'test', 'ucomis', 'push')):
            return allregs, set()
        if m.startswith(('mov', 'sqrt', 'lea', 'pop', 'cvt')):
            return set(REG.findall(' '.join(o[:-1]))), {dst}
        if m in ('xorpd', 'xorps', 'pxor') and o[0] == o[1]:
            return set(), {dst}
        return allregs, {dst}


def parse(lines):
    insts = []
    for line in lines:
        match = LINE.match(line)
        if match:
            insts.append(Inst(int(match.group(1), 16), match.group(2), ops_of(match.group(3))))
    return insts


def successors(insts, index_of, i):
    inst = insts[i]
    if inst.m in ('retq', 'ret', 'ud2'):
        return []
    if inst.m.startswith('j'):
        dest = index_of[int(inst.ops[0], 16)]
        return [dest] if inst.m in ('jmp', 'jmpq') else [dest, i + 1]
    return [i + 1] if i + 1 < len(insts) else []


def readers(insts, index_of, i, reg):
    """Every instruction that can read `reg` as left by instruction i.

    Returns None when the value may reach a return: the System V return
    registers and callee-saved registers are then considered read.
    """
    seen, out, work = set(), [], list(successors(insts, index_of, i))
    while work:
        j = work.pop()
        if j in seen or insts[j].dead:
            if j not in seen and insts[j].dead:
                seen.add(j)
                work.extend(successors(insts, index_of, j))
            continue
        seen.add(j)
        r, w = insts[j].reads_writes()
        if reg in r:
            out.append(j)
        if reg in w:
            continue
        if insts[j].m in ('retq', 'ret') and reg in ('%xmm0', '%xmm1', '%rax', '%rdx'):
            return None
        work.extend(successors(insts, index_of, j))
    return out


def slot_written_between(insts, i, j, slot):
    return j <= i or any(x.kind() == 'store' and x.ops[1] == slot and not x.dead for x in insts[i + 1:j])


def straight(insts, i, j):
    """No branch or branch target strictly between i and j (i < j)."""
    targets = {int(x.ops[0], 16) for x in insts if x.m.startswith('j')}
    return i < j and not any(x.m.startswith('j') or x.addr in targets for x in insts[i + 1:j]) \
        and insts[j].addr not in targets


def fold(insts, index_of):
    count = 0
    for i, inst in enumerate(insts):
        if inst.dead or inst.kind() != 'load':
            continue
        slot, reg = inst.ops
        rs = readers(insts, index_of, i, reg)
        if rs is None or len(rs) != 1:
            continue
        j = rs[0]
        use = insts[j]
        if not (ARITH.match(use.m) and len(use.ops) == 2 and use.ops[0] == reg and use.ops[1] != reg):
            continue
        if not straight(insts, i, j) or slot_written_between(insts, i, j, slot):
            continue
        use.ops[0] = slot
        inst.dead = True
        count += 1
    return count


def copies(insts, index_of):
    count = 0
    for i, inst in enumerate(insts):
        if inst.dead or inst.kind() != 'load':
            continue
        slot, reg = inst.ops
        rs = readers(insts, index_of, i, reg)
        if rs is None or len(rs) != 1:
            continue
        j = rs[0]
        use = insts[j]
        if use.kind() != 'copy' or use.ops[0] != reg:
            continue
        if not straight(insts, i, j) or slot_written_between(insts, i, j, slot):
            continue
        use.m, use.ops = inst.m, [slot, use.ops[1]]
        inst.dead = True
        count += 1
    return count


def stores(insts):
    """Value-number each straight-line region; drop stores of a slot's own value."""
    targets = {int(x.ops[0], 16) for x in insts if x.m.startswith('j')}
    val, fresh, count = {}, iter(range(1, 1 << 40)), 0
    for inst in insts:
        if inst.addr in targets:
            val = {}
        if inst.dead:
            continue
        kind = inst.kind()
        if kind == 'load':
            v = val.setdefault(('s', inst.ops[0]), next(fresh))
            val[('r', inst.ops[1])] = v
        elif kind == 'store':
            v = val.setdefault(('r', inst.ops[0]), next(fresh))
            if val.get(('s', inst.ops[1])) == v:
                inst.dead = True
                count += 1
            val[('s', inst.ops[1])] = v
        elif kind == 'copy':
            val[('r', inst.ops[1])] = val.setdefault(('r', inst.ops[0]), next(fresh))
        else:
            for w in inst.reads_writes()[1]:
                val[('r', w)] = next(fresh)
            if inst.m.startswith('call'):
                val = {}
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    for flag in ('fold', 'copies', 'stores', 'scalar'):
        parser.add_argument('--' + flag, action='store_true')
    parser.add_argument('--pad', type=int, default=0)
    parser.add_argument('--symbol', default='advance')
    args = parser.parse_args()
    insts = parse(sys.stdin)
    index_of = {inst.addr: i for i, inst in enumerate(insts)}
    done = {}
    if args.stores:
        done['stores'] = stores(insts)
    if args.copies:
        done['copies'] = copies(insts, index_of)
    if args.fold:
        done['fold'] = fold(insts, index_of)
    targets = {int(x.ops[0], 16) for x in insts if x.m.startswith('j')}
    out = ['.text', '.p2align 6']
    if args.pad:
        out.append(f'.skip {args.pad}, 0x90')
    out += [f'.globl {args.symbol}', f'.type {args.symbol},@function', f'{args.symbol}:']
    kept = 0
    for inst in insts:
        if inst.addr in targets:
            out.append(f'.L{inst.addr:x}:')
        if inst.dead:
            continue
        kept += 1
        m, ops = inst.m, list(inst.ops)
        if m.startswith('j'):
            ops = [f'.L{int(ops[0], 16):x}']
        elif args.scalar and inst.kind() in ('load', 'store') and m == 'movdqu':
            m = 'movsd'
        out.append(f'    {m} {", ".join(ops)}')
    out += [f'.size {args.symbol}, .-{args.symbol}', '.section .note.GNU-stack,"",@progbits']
    print('\n'.join(out))
    print(f'{kept} instructions kept; removed {done}', file=sys.stderr)


if __name__ == '__main__':
    main()
