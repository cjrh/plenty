#!/usr/bin/env python3
"""Build one Plenty source file into the isolated GPUI launcher (Linux x86-64)."""
import argparse
import os
from pathlib import Path
import platform
import shutil
import subprocess

PACKAGE = Path(__file__).resolve().parent
REPO = PACKAGE.parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="?", type=Path, default=PACKAGE / "examples/counter.plenty")
    parser.add_argument("--plenty", type=Path, default=REPO / "target/release/plenty")
    parser.add_argument("--fast-compile", action="store_true")
    parser.add_argument("--release", action="store_true", help="optimize the Rust launcher too")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("this experimental launcher is qualified only for Linux x86-64")
    compiler = args.plenty.resolve(strict=True)
    source = args.source.resolve(strict=True)
    profile = "fast-compile" if args.fast_compile else "optimized"
    out = PACKAGE / "target" / profile
    out.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, out / "application.plenty")
    shutil.copyfile(PACKAGE / "src/lib.plenty", out / "gpui.plenty")
    shutil.copyfile(PACKAGE / "src/native.plentyi", out / "native.plentyi")
    command = [str(compiler), "--shared-library", str(out / "application.plenty"),
               "--module-root", str(out), "--library-name", "gpui_app",
               "-o", str(out / "libgpui_app.so")]
    if args.fast_compile:
        command.append("--fast-compile")
    subprocess.run(command, check=True)
    env = {**os.environ, "PLENTY_GPUI_APP_DIR": str(out)}
    command = ["cargo", "build", "--locked", "--manifest-path", str(PACKAGE / "native/Cargo.toml"),
               "--features", "application"]
    if args.release:
        command.append("--release")
    subprocess.run(command, env=env, check=True)
    native_profile = "release" if args.release else "debug"
    print(PACKAGE / "native/target" / native_profile / "plenty-gpui", flush=True)


if __name__ == "__main__":
    main()
