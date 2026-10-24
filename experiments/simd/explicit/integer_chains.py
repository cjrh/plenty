#!/usr/bin/env python3
"""Compare identical Plenty integer kernels with two supplied compiler builds.

Usage: python3 integer_chains.py BASELINE_COMPILER SSE41_COMPILER > results.json
The second compiler is an experiment with only has_sse41 enabled; this script
does not change compiler source, ISA policy, or detect/enable CPU features.
"""
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent


def compile_program(compiler, source, output):
    subprocess.run(
        [compiler, "--release", "--compile", str(source), "-o", str(output)],
        env={**os.environ, "PLENTY_CACHE": "off"}, check=True,
        stdout=subprocess.DEVNULL,
    )


def measure(compiler, root):
    result = {"timings": {}, "counters": {}}
    for module in ["rounds", "integer_chains"]:
        source = root / (module + ".plenty")
        shutil.copyfile(HERE / source.name, source)
        binary = root / module
        compile_program(compiler, source, binary)
    # Finish compilation before collecting timing samples.
    wrappers = [("rounds", name) for name in ["scalar_rounds", "vector_rounds"]]
    wrappers += [("integer_chains", name) for name in ["scalar1", "vector1", "scalar2", "vector2"]]
    for module, function in wrappers:
        source = root / (function + ".plenty")
        arguments = "10000000, 0u32" + (", 1664525u32" if module == "integer_chains" else "")
        source.write_text(f"import {module}\ndef main() -> Result[(), IoError]:\n    print({module}.{function}({arguments}))?\n    Ok(())\n")
        compile_program(compiler, source, root / function)
    for module in ["rounds", "integer_chains"]:
        rows = [line.split() for line in subprocess.check_output([str(root / module)], text=True).splitlines()]
        cases = [1] if module == "rounds" else [1, 2]
        for case in cases:
            selected = rows if module == "rounds" else [r for r in rows if int(r[1]) == case]
            offset = 1 if module == "rounds" else 2
            scalar = statistics.median(int(r[offset]) for r in selected)
            vector = statistics.median(int(r[offset + 1]) for r in selected)
            result["timings"][f"{module}_{case}"] = {"rows": selected, "scalar_median_ns": scalar, "vector_median_ns": vector, "scalar_over_vector": scalar / vector}
    for _, function in wrappers:
        samples = []
        for _ in range(3):
            run = subprocess.run(["perf", "stat", "-x", ",", "-e", "instructions:u,cycles:u", str(root / function)], capture_output=True, text=True, check=True)
            samples.append({"checksum": run.stdout.strip(), "perf_csv": run.stderr.strip()})
        result["counters"][function] = samples
    return result


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="simd-integer-") as directory:
        root = Path(directory)
        print(json.dumps({label: measure(str(Path(compiler).resolve()), root) for label, compiler in zip(["baseline_sse2", "sse41_experiment"], sys.argv[1:], strict=True)}, indent=2))
