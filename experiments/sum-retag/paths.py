#!/usr/bin/env python3
"""List every path from entry to `ret` through a loop-free function.

Reads harness output (`addr:  mnemonic operands`) or `objdump -d` output
on stdin. For each function it prints one line per path: the number of
instructions the path runs, its calls, and the last instruction that sets
the returned high word (%rcx for Plenty's `tail` convention, %rdx for C),
which shows the variant the path returns. The paths are not checked for
feasibility: two tests of the same bit give some paths that cannot run.
Paths ending in `ud2` (traps) are left out. See README.md.
"""
import re
import sys

LINE = re.compile(r"^\s*([0-9a-fA-F]+):\s+([a-z][\w.]*)\s*(.*)$")
CONDITIONAL = re.compile(r"^j(?!mp)[a-z]+$")


def functions(lines):
    name, insns = None, []
    for line in lines:
        header = re.match(r"^==== (?:\S+ )?(\S+)", line) or re.match(r"^[0-9a-f]+ <(\S+)>:", line)
        if header:
            if name:
                yield name, insns
            name, insns = header.group(1), []
            continue
        match = LINE.match(line)
        if match and name:
            operands = match.group(3).split("#")[0].strip()
            insns.append((int(match.group(1), 16), match.group(2), operands))
    if name:
        yield name, insns


def target(operands):
    match = re.match(r"^(?:0x)?([0-9a-fA-F]+)(?:\s+<[^>]*>)?$", operands)
    return int(match.group(1), 16) if match else None


def paths(insns):
    index = {address: i for i, (address, _, _) in enumerate(insns)}
    stack = [(0, [])]
    while stack:
        i, path = stack.pop()
        while i < len(insns):
            address, mnemonic, operands = insns[i]
            path = path + [i]
            if mnemonic in ("ret", "retq"):
                yield path
                break
            if mnemonic == "ud2":
                break
            if mnemonic in ("jmp", "jmpq"):
                i = index[target(operands)]
                continue
            if CONDITIONAL.match(mnemonic):
                stack.append((index[target(operands)], path))
            i += 1


def writes(insn, register):
    """Whether `insn` (AT&T syntax) writes the 64-bit `register`."""
    _, mnemonic, operands = insn
    if mnemonic.startswith(("cmp", "test", "push", "j", "call")):
        return False
    destination = operands.rsplit(",", 1)[-1].strip()
    names = {"rcx": ("%rcx", "%ecx", "%cx", "%cl"), "rdx": ("%rdx", "%edx", "%dx", "%dl")}
    return destination in names[register]


def main():
    register = sys.argv[1] if len(sys.argv) > 1 else "rcx"
    for name, insns in functions(sys.stdin):
        print(f"== {name}: {len(insns)} instructions")
        for path in sorted(paths(insns), key=len):
            calls = [insns[i][2] for i in path if insns[i][1] in ("call", "callq")]
            high = next(
                (f"{insns[i][1]} {insns[i][2]}" for i in reversed(path) if writes(insns[i], register)),
                "unset",
            )
            print(f"{len(path):4}  calls={len(calls)}  high word: {high}")


if __name__ == "__main__":
    main()
