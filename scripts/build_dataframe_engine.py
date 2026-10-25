#!/usr/bin/env python3
"""Build/install the optional std.dataframe engine once, outside normal builds."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
LIBRARY = "libplenty_dataframe_v1.so"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True, help="installation directory (e.g. target/release beside plenty)")
    parser.add_argument("--target-dir", type=Path, default=ROOT / "target" / "dataframe-engine")
    parser.add_argument("--jobs", type=int, default=3)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    version = subprocess.check_output(["rustc", "-vV"], text=True)
    if "host: x86_64-unknown-linux-gnu" not in version:
        parser.error("the packaged dataframe engine currently supports x86_64-unknown-linux-gnu")
    env = {key: value for key, value in os.environ.items()
           if not (key.startswith(("CFLAGS", "CXXFLAGS", "TARGET_CFLAGS", "TARGET_CXXFLAGS")))}
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    env.update(CARGO_TARGET_DIR=str(args.target_dir.resolve()),
               CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="cc",
               RUSTFLAGS=f"-C target-cpu=x86-64 -C link-arg=-Wl,-soname,{LIBRARY}",
               CFLAGS="-march=x86-64 -mtune=generic",
               CXXFLAGS="-march=x86-64 -mtune=generic")
    subprocess.run(["cargo", "build", "--manifest-path", str(ROOT / "plenty-dataframe" / "Cargo.toml"),
                    "--locked", "--release", "--jobs", str(args.jobs)], env=env, check=True, cwd=ROOT)
    args.out.mkdir(parents=True, exist_ok=True)
    source = args.target_dir.resolve() / "release" / LIBRARY
    destination = args.out.resolve() / LIBRARY
    if source != destination:
        temporary = destination.with_suffix(f".tmp-{os.getpid()}")
        shutil.copy2(source, temporary)
        temporary.replace(destination)
    print(f"Installed baseline x86-64 dataframe engine: {destination}")
    print("The compiler discovers it beside its executable, or via PLENTY_DATAFRAME_ENGINE.")


if __name__ == "__main__":
    main()
