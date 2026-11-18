#!/usr/bin/env python3
"""Build one Plenty source file into the single Plenty executable with opt-in iced support (Linux x86-64)."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

PACKAGE = Path(__file__).resolve().parent
REPO = PACKAGE.parents[2]


# Reviewed whole-interface pin, independent of the staged application's header.
EXPECTED_CONTRACT = (PACKAGE / "native/legacy/expected-contract.txt").read_text().strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="?", type=Path, default=PACKAGE / "examples/counter.plenty")
    parser.add_argument("--plenty", type=Path, default=REPO / "target/release/plenty")
    parser.add_argument("--fast-compile", action="store_true")
    parser.add_argument("--release", action="store_true", help="optimize the Rust native support archive too")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("this experimental launcher is qualified only for Linux x86-64")
    compiler = args.plenty.resolve(strict=True)
    source = args.source.resolve(strict=True)
    profile = "fast-compile" if args.fast_compile else "optimized"
    out = PACKAGE / "target" / profile
    out.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, out / "application.plenty")
    shutil.copyfile(PACKAGE / "src/lib.plenty", out / "iced.plenty")
    shutil.copyfile(PACKAGE / "src/native.plentyi", out / "native.plentyi")
    command = [str(compiler), "--emit-object", str(out / "application.plenty"),
               "--module-root", str(out), "--executable-exports", "iced_app",
               "-o", str(out / "application.o")]
    if args.fast_compile:
        command.append("--fast-compile")
    subprocess.run(command, check=True)
    env = {**os.environ, "PLENTY_ICED_APP_DIR": str(out), "CARGO_NET_OFFLINE": "true"}
    executable = out / "plenty-iced"
    command = [sys.executable, "-B", REPO / "scripts/link_native_executable.py",
               "--manifest-path", PACKAGE / "native/Cargo.toml", "--lib-name", "plenty_iced",
               "--target-dir", PACKAGE / "native/target", "--features", "legacy-application",
               "--require-contract", EXPECTED_CONTRACT, "-o", executable, out / "application.o"]
    if args.release:
        command.append("--release")
    result = subprocess.run(command, env=env)
    if result.returncode:
        raise SystemExit(result.returncode)
    print(executable, flush=True)


if __name__ == "__main__":
    main()
