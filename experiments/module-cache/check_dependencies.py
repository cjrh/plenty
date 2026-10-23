#!/usr/bin/env python3
"""Show why an imported interface needs facts inferred from function bodies."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="plenty-module-facts-") as temp:
        root = Path(temp)
        (root / "main.plenty").write_text(
            "import data\n"
            "def main() -> Result[(), IoError]:\n"
            "    mut pair = data.Pair(1, 2)\n"
            "    value = data.select(&mut pair)\n"
            "    pair.y = 3\n"
            "    print(*value)?\n"
            "    Ok(())\n")
        for field in ["x", "y", "x"]:
            (root / "data.plenty").write_text(
                "pub class Pair:\n    pub x: i64\n    pub y: i64\n"
                "pub def select(pair: &mut Pair) -> &mut i64:\n"
                f"    &mut pair.{field}\n")
            result = subprocess.run(
                [str(compiler), "--check", str(root / "main.plenty")],
                env=dict(os.environ, PLENTY_CACHE="off"), capture_output=True, text=True)
            if field == "x":
                assert result.returncode == 0, result.stderr
            else:
                assert result.returncode != 0 and "conflicting borrow" in result.stderr, result.stderr
        print("same declarations: x body accepted, y body rejects caller, reverting restores acceptance")


if __name__ == "__main__":
    main()
