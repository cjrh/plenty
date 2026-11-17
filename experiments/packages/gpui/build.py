#!/usr/bin/env python3
"""Build one Plenty source file into the single Plenty executable with opt-in GPUI support (Linux x86-64)."""
import argparse
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess

PACKAGE = Path(__file__).resolve().parent
REPO = PACKAGE.parents[2]


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
    shutil.copyfile(PACKAGE / "src/lib.plenty", out / "gpui.plenty")
    shutil.copyfile(PACKAGE / "src/native.plentyi", out / "native.plentyi")
    command = [str(compiler), "--emit-object", str(out / "application.plenty"),
               "--module-root", str(out), "--executable-exports", "gpui_app",
               "-o", str(out / "application.o")]
    if args.fast_compile:
        command.append("--fast-compile")
    subprocess.run(command, check=True)
    env = {**os.environ, "PLENTY_GPUI_APP_DIR": str(out)}
    command = ["cargo", "rustc", "--locked", "--manifest-path", str(PACKAGE / "native/Cargo.toml"),
               "--target-dir", str(PACKAGE / "native/target"), "--lib", "--features", "application"]
    if args.release:
        command.append("--release")
    command += ["--", "--print", "native-static-libs"]
    result = subprocess.run(command, env=env, text=True, capture_output=True)
    (out / "native-build.log").write_text(result.stdout + result.stderr)
    if result.returncode:
        raise SystemExit(f"native support build failed; see {out / 'native-build.log'}")
    reports = [line.removeprefix("note: native-static-libs: ") for line in result.stderr.splitlines()
               if line.startswith("note: native-static-libs: ")]
    if len(reports) != 1:
        raise SystemExit("rustc did not report exactly one native-static-libs list; see native-build.log")
    native_args = shlex.split(reports[0])
    (out / "native-link-args.txt").write_text("\n".join(native_args) + "\n")
    native_profile = "release" if args.release else "debug"
    archive = PACKAGE / "native/target" / native_profile / "libplenty_gpui.a"
    executable = out / "plenty-gpui"
    # Exactly one Rust-built archive: never link the compiler's bundled runtime.
    command = ["cc", str(out / "application.o"), str(archive), "-Wl,--gc-sections",
               *native_args, "-o", str(executable)]
    (out / "link-command.txt").write_text(shlex.join(command) + "\n")
    subprocess.run(command, check=True)
    print(executable, flush=True)


if __name__ == "__main__":
    main()
