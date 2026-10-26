#!/usr/bin/env python3
"""Compatibility entry point for the dataframe release archive."""
import runpy
from pathlib import Path

engine = runpy.run_path(str(Path(__file__).with_name("package-engine.py")))
check_exports = engine["check_exports"]

if __name__ == "__main__":
    engine["main"]("dataframe")
