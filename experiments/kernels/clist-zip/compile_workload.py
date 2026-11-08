#!/usr/bin/env python3
"""Export compile_bench's 1,000-unit source for before/after compiler CLI counts."""
from pathlib import Path

root = Path(__file__).resolve().parents[3]
benchmark = (root / 'examples/compile_bench.rs').read_text()
template = 'protocol Measured{i}:' + benchmark.split('"protocol Measured{i}:', 1)[1].split('\n"', 1)[0]
source = ''.join(template.replace('{i}', str(i)) for i in range(1000))
source += ('def main() -> Result[(), IoError | AllocError | OverflowError[i64] | ZeroDivisionError]:\n'
           '    print(work999(5)?)?\n    Ok(())\n')
print(source, end='')
