#!/usr/bin/env python3
"""Write programs the experiments' generators produce into OUTPUT.

`tests/test_experiments.rs` type-checks every `.plenty` file written here, so
a language change that breaks a generator's output fails the suite. Each call
uses small representative sizes; the experiments' own runners choose the
measured ones.
"""
import importlib.util
from pathlib import Path
import sys

sys.dont_write_bytecode = True
EXPERIMENTS = Path(__file__).resolve().parents[3] / "experiments"


def load(relative):
    path = EXPERIMENTS / relative
    spec = importlib.util.spec_from_file_location(path.stem, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    output = Path(sys.argv[1])

    def generated(name, source):
        (output / f"{name}.plenty").write_text(source)

    sys.path.insert(0, str(EXPERIMENTS / "columnar-list/plenty"))
    import generate
    import bench
    import count_access
    import probe_scan_layout
    generated("columns", generate.generate(
        (EXPERIMENTS / "columnar-list/plenty/particle.plenty").read_text()))
    for native, skip in [(False, False), (True, False), (True, True)]:
        generated(f"columns_{native}_{skip}", bench.program(16, 2, 1, native, skip))
    for kernel in ["one", "two", "stream", "whole", "random"]:
        generated(f"count_{kernel}", count_access.source(16, 2, kernel))
    generated("scalar_layout", probe_scan_layout.scalar_source(16, 2))
    removal = load("columnar-list/removal.py")
    for fields in [1, 8]:
        generated(f"removal_{fields}", removal.program(fields, 16, 2, 1))

    # Each variant's modules import one another, so they share a directory.
    modules = load("module-cache/run.py")
    for variant in ["base", "body", "interface"]:
        directory = output / variant
        directory.mkdir()
        modules.fixtures(directory, 2, 3, variant)

    # The workload alone lacks the foreign API its runner selects.
    crates = load("rust-crates/run.py")
    generated("rust_crates", crates.checked_source(
        (EXPERIMENTS / "rust-crates/workload.plenty").read_text()))
    # emitter/bench imports its sibling run module.
    sys.path.insert(0, str(EXPERIMENTS / "rust-source-backend/emitter"))
    emitter = load("rust-source-backend/emitter/bench.py")
    generated("emitter", emitter.program(3, 2))


if __name__ == "__main__":
    main()
