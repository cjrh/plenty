#!/usr/bin/env python3
"""Compile/run native evidence and reject syntax outside the experiment."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from lower import Unsupported, lower

ROOT = Path(__file__).resolve().parent


class SpikeTests(unittest.TestCase):
    def test_native_state_machines(self):
        generated = lower((ROOT / "program.async").read_text())
        with tempfile.TemporaryDirectory(prefix="plenty-async-spike-") as directory:
            source = Path(directory) / "check.c"
            executable = Path(directory) / "check"
            source.write_text(generated + (ROOT / "check.c").read_text())
            flags = ["-std=c11", "-O1", "-g", "-Wall", "-Wextra", "-Werror",
                     "-Wno-unused-function"]
            if os.environ.get("SANITIZE") == "1":
                flags += ["-fsanitize=address,undefined", "-fno-omit-frame-pointer"]
            subprocess.run([os.environ.get("CC", "cc"), *flags, "-I", str(ROOT),
                            str(source), "-o", str(executable)], check=True)
            subprocess.run([str(executable)], check=True)

    def test_rejected_forms(self):
        cases = {
            "borrowed parameter": "async def f(x: Ref) -> i64:\n    return 0\n",
            "recursive frame": "async def f(x: i64) -> i64:\n    y = await f(x)\n    return y\n",
            "async with": "async def f() -> i64:\n    async with x:\n        pass\n    return 0\n",
            "loops": "async def f() -> i64:\n    while True:\n        await tick(0)\n    return 0\n",
            "resource escape": "async def f() -> i64:\n    x = acquire()\n    return x\n",
            "resource copy": "async def f() -> i64:\n    x = acquire()\n    y = x\n    return 0\n",
            "resource overwrite": "async def f() -> i64:\n    x = acquire()\n    x = 2\n    return 0\n",
            "future as scalar": "async def f() -> i64:\n    x = tick(0)\n    return x\n",
            "unknown target": "async def f() -> i64:\n    x = await unknown(0)\n    return x\n",
            "wrong arity": "async def f() -> i64:\n    x = await tick()\n    return x\n",
            "uninitialized local": "async def f() -> i64:\n    return x\n",
            "synchronous def": "def f() -> i64:\n    return 0\n",
        }
        for label, source in cases.items():
            with self.subTest(label=label), self.assertRaises(Unsupported):
                lower(source)


if __name__ == "__main__":
    unittest.main()
