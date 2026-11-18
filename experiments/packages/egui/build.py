#!/usr/bin/env python3
"""Build one Plenty source file into the single Plenty executable with opt-in egui support (Linux x86-64)."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

PACKAGE = Path(__file__).resolve().parent
REPO = PACKAGE.parents[2]


# Reviewed ownership contract, independent of the header generated for this build.
GUARD = "egui_app_plenty_contract_v1_89958b2fdbfd0d0f069c1bc193892a0a85ea81d6e227fd12a11fe40d3950d06f"


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
    shutil.copyfile(PACKAGE / "src/lib.plenty", out / "egui.plenty")
    shutil.copyfile(PACKAGE / "src/native.plentyi", out / "native.plentyi")
    command = [str(compiler), "--emit-object", str(out / "application.plenty"),
               "--module-root", str(out), "--executable-exports", "egui_app",
               "-o", str(out / "application.o")]
    if args.fast_compile:
        command.append("--fast-compile")
    subprocess.run(command, check=True)
    executable = out / "plenty-egui"
    link = [sys.executable, REPO / "scripts/link_native_executable.py",
            "--manifest-path", PACKAGE / "native/Cargo.toml", "--lib-name", "plenty_egui",
            "--target-dir", PACKAGE / "native/target", "--features", "application",
            "--require-contract", GUARD, "-o", executable, out / "application.o"]
    if args.release:
        link.append("--release")
    subprocess.run(link, check=True, env={**os.environ, "PLENTY_EGUI_APP_DIR": str(out),
                                         "CARGO_NET_OFFLINE": "true"})
    print(executable, flush=True)


if __name__ == "__main__":
    main()
