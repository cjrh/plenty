#!/usr/bin/env python3
"""Check experiment inputs and generated fixtures with the current Plenty compiler.

This is a syntax/contracts smoke check, not a rerun of recorded measurements or
of foreign-library builds. Run each experiment's own runner for those checks.
"""
import argparse
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib

HERE = Path(__file__).resolve().parent


def load(relative):
    path = HERE / relative
    spec = importlib.util.spec_from_file_location(path.stem, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    env = dict(os.environ, PLENTY_CACHE="off")
    count = 0

    def check(path, diagnostic=None):
        nonlocal count
        result = subprocess.run([str(compiler), "--check-module", str(path)],
                                env=env, capture_output=True, text=True, timeout=60)
        if diagnostic:
            assert result.returncode and diagnostic in result.stderr, result.stderr
        else:
            assert result.returncode == 0, f"{path}: {result.stderr}"
        count += 1

    def check_package(directory, label):
        nonlocal count
        result = subprocess.run([str(compiler), "check", "--offline"], cwd=directory,
                                env=env, capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, f"{label}: {result.stderr}"
        count += 1

    def tracked(path):
        return "target" not in path.relative_to(HERE).parts

    # A package's modules and tests resolve imports through its manifest, not
    # relative to each file, so they are checked as packages below (issue #910).
    packages = sorted(manifest.parent for manifest in HERE.rglob("plenty.toml")
                      if tracked(manifest))

    # Only tracked input directories; ignore build products left by other runs.
    for path in sorted(HERE.rglob("*.plenty")):
        if not tracked(path) or any(path.is_relative_to(package) for package in packages):
            continue
        relative = path.relative_to(HERE).as_posix()
        if relative == "rust-crates/workload.plenty":
            continue  # Its runner supplies the selected foreign API stubs below.
        rejection = ("references cannot be stored in class fields"
                     if relative == "columnar-list/reject-row-reference.plenty" else None)
        check(path, rejection)

    with tempfile.TemporaryDirectory(prefix="plenty-experiment-contracts-") as temporary:
        work = Path(temporary)

        for package in packages:
            check_package(package, package.relative_to(HERE).as_posix())
            # `plenty check` covers the library, not `tests/`. `plenty test`
            # would also run the tests, which need the package's native
            # library built. Each test file instead becomes the library of a
            # consumer package with a path dependency, so `import NAME`
            # resolves as it does under `plenty test`. Issue #919 tracks a
            # check command for tests that would replace this.
            manifest = tomllib.loads((package / "plenty.toml").read_text())["package"]
            for test in sorted((package / "tests").glob("*.plenty")):
                consumer = work / "packages" / package.name / test.stem
                (consumer / "src").mkdir(parents=True)
                (consumer / "src/lib.plenty").write_text(test.read_text())
                (consumer / "plenty.toml").write_text(
                    "manifest-version = 1\n\n[package]\n"
                    f"name = \"{manifest['name']}_{test.stem}\"\nversion = \"0.0.0\"\n\n"
                    f"[dependencies]\n{manifest['name']} = {{ path = \"{package}\", "
                    f"version = \"{manifest['version']}\" }}\n\n"
                    "[lib]\npath = \"src/lib.plenty\"\n")
                check_package(consumer, test.relative_to(HERE).as_posix())

        def generated(name, source):
            path = work / f"{name}.plenty"
            path.write_text(source)
            check(path)

        sys.path.insert(0, str(HERE / "columnar-list/plenty"))
        import generate
        import bench
        import count_access
        generated("columns", generate.generate((HERE / "columnar-list/plenty/particle.plenty").read_text()))
        for native, skip in [(False, False), (True, False), (True, True)]:
            generated(f"columns_{native}_{skip}", bench.program(16, 2, 1, native, skip))
        for kernel in ["one", "two", "stream", "whole", "random"]:
            generated(f"count_{kernel}", count_access.source(16, 2, kernel))
        import probe_scan_layout
        generated("scalar_layout", probe_scan_layout.scalar_source(16, 2))
        removal = load("columnar-list/removal.py")
        for fields in [1, 8]:
            generated(f"removal_{fields}", removal.program(fields, 16, 2, 1))

        modules = load("module-cache/run.py")
        for variant in ["base", "body", "interface"]:
            directory = work / variant
            directory.mkdir()
            modules.fixtures(directory, 2, 3, variant)
            for path in directory.glob("*.plenty"):
                check(path)

        crates = load("rust-crates/run.py")
        generated("rust_crates", crates.checked_source((HERE / "rust-crates/workload.plenty").read_text()))
        # emitter/bench imports its sibling run module.
        sys.path.insert(0, str(HERE / "rust-source-backend/emitter"))
        emitter = load("rust-source-backend/emitter/bench.py")
        generated("emitter", emitter.program(3, 2))
    print(f"PASS: {count} experiment inputs and generated fixtures")


if __name__ == "__main__":
    main()
