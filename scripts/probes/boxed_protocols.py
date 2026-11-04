#!/usr/bin/env python3
"""Reproduce boxed-protocol behavior, or the historical pre-implementation baseline.

Use --baseline only with the research-era compiler. Default probes exercise
explicit protocol objects and preserve concrete-box and callable regressions.
Pass a compiler built from the checkout being investigated.
"""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap


DECLARATIONS = """
protocol MyProtocol:
    def hey(self) -> i64:
        pass

class A:
    value: i64

    def hey(self) -> i64:
        self.value

enum B:
    Value(i64)

    def hey(self) -> i64:
        match self:
            case B.Value(value):
                *value
"""


def cases(baseline=False):
    yield (
        "protocol-box-storage",
        DECLARATIONS + ("""
def main() -> Result[(), AllocError]:
    items: list[Box[MyProtocol]] = []?
    Ok(())
""" if baseline else """
def main() -> Result[(), AllocError | IoError]:
    items: list[Box[MyProtocol]] = [Box[MyProtocol](A(10))?, Box[MyProtocol](B.Value(20))?]?
    for item in &items:
        print(item.hey())?
    Ok(())
"""),
        None if baseline else "10\n20\n",
        "unknown type `MyProtocol`" if baseline else None,
    )
    concrete = DECLARATIONS + """
def main() -> Result[(), AllocError | IoError]:
    mut items: list[Box[A]] = []?
    items.append(Box(A(10))?)?
    for item in &items:
        print(item.hey())?
    Ok(())
"""
    yield "concrete-box-method-through-borrowed-list", concrete, "10\n", None
    yield (
        "boxing-does-not-erase-concrete-type",
        concrete.replace(
            "    for item in &items:",
            "    items.append(Box(B.Value(20))?)?\n    for item in &items:",
        ),
        None,
        "expected Box[A], got Box[B]",
    )
    yield (
        "protocol-checked-single-method-erasure",
        DECLARATIONS + """
def erase[T: MyProtocol](value: T) -> Result[BoxedCallable[[], i64], AllocError]:
    callback = def [value]() -> i64:
        value.hey()
    BoxedCallable(callback)

def main() -> Result[(), AllocError | IoError]:
    mut items: list[BoxedCallable[[], i64]] = []?
    items.append(erase(A(10))?)?
    items.append(erase(B.Value(20))?)?
    for item in &items:
        print(item())?
    Ok(())
""",
        "10\n20\n",
        None,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--baseline", action="store_true")
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    passed = 0
    with tempfile.TemporaryDirectory(prefix="plenty-boxed-protocol-probes-") as temp:
        for name, source, expected, diagnostic in cases(args.baseline):
            path = Path(temp) / (name + ".plenty")
            path.write_text(textwrap.dedent(source).lstrip())
            command = [str(compiler)]
            if diagnostic is not None:
                command.append("--check")
            command.append(str(path))
            result = subprocess.run(
                command, capture_output=True, text=True, timeout=60,
                env={**os.environ, "PLENTY_CACHE": "off"},
            )
            if diagnostic is None:
                ok = result.returncode == 0 and result.stdout == expected
            else:
                ok = result.returncode == 1 and diagnostic in result.stderr
            if not ok:
                raise SystemExit(
                    f"FAIL {name} (exit {result.returncode})\n"
                    f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
                )
            print(f"PASS {name}")
            passed += 1
    print(f"{passed} probes passed")


if __name__ == "__main__":
    main()
