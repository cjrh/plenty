#!/usr/bin/env python3
"""Convert the baseline velocity dump into before/after CLIF attribution probes.

Run against the pre-change dump; the after variant assumes both observed
increments follow successful bounds checks, as verified for this kernel.
This is a diagnostic transformation, not a general CLIF optimizer.
"""
from pathlib import Path
import re
import sys

source, output = map(Path, sys.argv[1:])
output.mkdir(parents=True, exist_ok=True)
text = source.read_text().split("\n", 1)[1]
text = re.sub(r"function u\d+:\d+\(", "function %velocity(", text)
text = text.replace("userextname", "u0:")
(output / "before.clif").write_text(text)
changed, count = re.subn(
    r"(v\d+), (v\d+) = sadd_overflow.i64 (v\d+), (v\d+)",
    r"\1 = iadd.i64 \3, \4\n    \2 = iconst.i8 0", text)
if count != 2:
    raise RuntimeError(f"expected the two diagnosed counter increments, got {count}")
(output / "after.clif").write_text(changed)
