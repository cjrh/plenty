#!/usr/bin/env python3
"""Compare recorded split/merged matrix IR with two explicit CLIF harnesses.

Only the cold source-diagnostic helper is replaced with a trap, to permit raw
linking. This checks valid descriptors, IEEE results and memory access; Plenty
integration tests must separately check actual diagnostics and borrow rules.
"""
import argparse
import os
from pathlib import Path
import platform
import re
import subprocess


def export(harness, inputs, output):
    output.mkdir(parents=True, exist_ok=True)
    sources = []
    for i, source in enumerate(inputs):
        text = source.read_text()
        # The harness refuses remaining external relocations rather than
        # silently linking an incomplete function.
        text = re.sub(r"(block\d+ cold:)\n.*?(?=\nblock|\n})",
                      r"\1\n    trap user3\n", text, flags=re.S)
        path = output / f"{i}.clif"
        path.write_text(text)
        sources.append(path)
    env = dict(os.environ, PLENTY_KERNEL_CODE_DIR=str(output / "code"))
    # Baseline SSE2 only, independently of the caller's optional ISA settings.
    env.pop("PLENTY_KERNEL_AVX", None)
    env.pop("PLENTY_KERNEL_AVX2", None)
    result = subprocess.run([harness, *sources], env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    (output / "harness.log").write_text(result.stdout)
    result.check_returncode()
    return [output / "code" / f"{i}-0.bin" for i in range(len(inputs))]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-harness", required=True, type=Path)
    parser.add_argument("--candidate-harness", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("raw-kernel calling convention probe requires Linux x86-64")
    source = Path(__file__).resolve().parent
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    before = export(args.baseline_harness.resolve(),
                    [source / "baseline-split.clif", source / "at-baseline-split.clif"],
                    output / "baseline")
    after = export(args.candidate_harness.resolve(),
                   [source / "a_times_u-coalesced.clif", source / "at_times_u-coalesced.clif"],
                   output / "candidate")
    assembly = ".text\n"
    for name, path in zip(["baseline_a", "baseline_at", "candidate_a", "candidate_at"],
                          before + after):
        escaped = str(path).replace("\\", "\\\\").replace('"', '\\"')
        assembly += (f".p2align 4\n.globl {name}\n.type {name}, @function\n{name}:\n"
                     f'.incbin "{escaped}"\n.size {name}, .-{name}\n')
    assembly += '.section .note.GNU-stack,"",@progbits\n'
    asm = output / "kernels.s"
    asm.write_text(assembly)
    binary = output / "verify"
    subprocess.run(["cc", "-O2", "-frounding-math", "-Wall", "-Wextra",
                    source / "verify.c", asm, "-lm", "-o", binary], check=True)
    result = subprocess.run([binary], text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT)
    (output / "verification.log").write_text(result.stdout)
    print(result.stdout, end="")
    result.check_returncode()


if __name__ == "__main__":
    main()
