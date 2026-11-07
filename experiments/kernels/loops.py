#!/usr/bin/env python3
"""Count the instructions of each innermost loop in a disassembly.

Reads the harness output (`addr:  mnemonic operands`) or `objdump -d
--no-show-raw-insn` output on stdin and prints, per innermost loop, the
instructions one iteration runs on its hot path when every conditional
branch inside it either stays in the loop or exits it.
"""
import re
import sys

LINE = re.compile(r"^\s*([0-9a-f]+):\s+(?:[0-9a-f]{2} )*\s*([a-z][\w.]*)\s*(.*)$")


def parse(lines):
    insns, func = [], None
    for line in lines:
        # `plenty --dump asm` names the function first, then `symbol=`; the
        # CLIF harness prints its input file, then the function.
        header = re.match(
            r"^(?:==== (\S+)\s+symbol=|==== \S+ (\S+)|[0-9a-f]+ <(\S+)>:)", line
        )
        if header:
            func = header.group(1) or header.group(2) or header.group(3)
            continue
        # `--dump asm` prints zero-padded uppercase jump targets.
        line = re.sub(r"0x([0-9A-Fa-f]+)", lambda m: hex(int(m.group(1), 16)), line)
        match = LINE.match(line)
        if match and func:
            insns.append((func, int(match.group(1), 16), match.group(2), match.group(3)))
    return insns


def target(op):
    match = re.match(r"^(?:0x)?([0-9a-f]+)\b", op.strip())
    return int(match.group(1), 16) if match else None


UNCONDITIONAL = ("ret", "retq", "jmp", "jmpq", "ud2")


def loops(insns):
    """Yield each innermost natural loop's hot instructions.

    Blocks are split at branches and branch targets. A loop is a backward
    branch with its target; its body is the blocks in that address range
    that reach the branch, so exit paths and cold blocks placed after the
    hot code are excluded.
    """
    by_func = {}
    for insn in insns:
        by_func.setdefault(insn[0], []).append(insn[1:])
    for func, code in by_func.items():
        dests = {target(op) for _, m, op in code if m.startswith("j")}
        blocks, current = [], []
        for addr, mnemonic, op in code:
            if addr in dests and current:
                blocks.append(current)
                current = []
            current.append((addr, mnemonic, op))
            if mnemonic.startswith("j") or mnemonic in UNCONDITIONAL:
                blocks.append(current)
                current = []
        if current:
            blocks.append(current)
        starts = [block[0][0] for block in blocks]

        def successors(index):
            addr, mnemonic, op = blocks[index][-1]
            out = []
            if mnemonic.startswith("j") and target(op) in starts:
                out.append(starts.index(target(op)))
            if mnemonic not in UNCONDITIONAL and index + 1 < len(blocks):
                out.append(index + 1)
            return out

        ranges = []
        for index, block in enumerate(blocks):
            addr, mnemonic, op = block[-1]
            dest = target(op) if mnemonic.startswith("j") else None
            if dest is not None and dest <= addr and dest in starts:
                ranges.append((starts.index(dest), index))
        inner = [r for r in ranges
                 if not any(o != r and r[0] <= o[0] and o[1] <= r[1] for o in ranges)]
        for head, tail in inner:
            # Blocks in range from which `tail` is reachable within the range.
            body = {tail}
            changed = True
            while changed:
                changed = False
                for index in range(head, tail + 1):
                    if index not in body and any(s in body for s in successors(index)):
                        body.add(index)
                        changed = True
            if head not in body:
                continue
            hot = [insn for index in sorted(body) for insn in blocks[index]]
            yield func, starts[head], blocks[tail][-1][0], hot


def main():
    verbose = "-v" in sys.argv
    for func, start, end, body in loops(parse(sys.stdin)):
        print(f"{func} loop {start:#x}-{end:#x}: {len(body)} instructions")
        if verbose:
            for addr, mnemonic, op in body:
                print(f"    {addr:6x}  {mnemonic} {op}")


if __name__ == "__main__":
    main()
