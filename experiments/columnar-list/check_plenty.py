#!/usr/bin/env python3
"""Check the current-language prototype and two expected baseline rejections."""
import os
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parent
compiler = str(Path(sys.argv[1]).resolve())
env = os.environ.copy()
env["PLENTY_CACHE"] = "off"
cases = [
    ("columns.plenty", [], 0, "90\n"),
    ("reject-row-reference.plenty", ["--check"], 1, "references cannot be stored in class fields"),
    ("reject-clist.plenty", ["--check"], 1, "`clist` is not a generic function"),
]
for name, options, expected_code, expected in cases:
    result = subprocess.run([compiler, *options, str(root / name)], env=env, text=True, capture_output=True)
    assert result.returncode == expected_code, (name, result.returncode, result.stderr)
    actual = result.stdout if expected_code == 0 else result.stderr
    assert (actual == expected if expected_code == 0 else expected in actual), (name, actual)
    print(f"PASS {name}")
