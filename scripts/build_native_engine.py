#!/usr/bin/env python3
"""Build/install an optional native engine once, outside normal compiler builds."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def main(engine=None):
    parser = argparse.ArgumentParser(description=__doc__)
    if engine is None:
        parser.add_argument("name", choices=("dataframe", "linalg"))
    parser.add_argument("--out", type=Path, required=True, help="installation directory (e.g. target/release beside plenty)")
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--jobs", type=int, default=3)
    args = parser.parse_args()
    name = engine or args.name
    library = f"libplenty_{name}_v1.so"
    args.target_dir = args.target_dir or ROOT / "target" / f"{name}-engine"
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    version = subprocess.check_output(["rustc", "-vV"], text=True)
    if "host: x86_64-unknown-linux-gnu" not in version:
        parser.error("packaged engines currently support x86_64-unknown-linux-gnu")
    env = {key: value for key, value in os.environ.items()
           if not (key.startswith(("CFLAGS", "CXXFLAGS", "TARGET_CFLAGS", "TARGET_CXXFLAGS")))}
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    env.update(CARGO_TARGET_DIR=str(args.target_dir.resolve()),
               CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="cc",
               RUSTFLAGS=f"-C target-cpu=x86-64 -C link-arg=-Wl,-soname,{library}",
               CFLAGS="-march=x86-64 -mtune=generic",
               CXXFLAGS="-march=x86-64 -mtune=generic")
    subprocess.run(["cargo", "build", "--manifest-path", str(ROOT / f"plenty-{name}" / "Cargo.toml"),
                    "--locked", "--release", "--jobs", str(args.jobs)], env=env, check=True, cwd=ROOT)
    args.out.mkdir(parents=True, exist_ok=True)
    source = args.target_dir.resolve() / "release" / library
    destination = args.out.resolve() / library
    if source != destination:
        temporary = destination.with_suffix(f".tmp-{os.getpid()}")
        shutil.copy2(source, temporary)
        temporary.replace(destination)
    print(f"Installed baseline x86-64 {name} engine: {destination}")
    print(f"The compiler discovers it beside its executable, or via PLENTY_{name.upper()}_ENGINE.")


if __name__ == "__main__":
    main()
