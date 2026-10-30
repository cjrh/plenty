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
    parser.add_argument("--target", choices=("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"))
    parser.add_argument("--validate", action="store_true", help="also format-check, test, and lint the engine with the selected baseline")
    parser.add_argument("--jobs", type=int, default=3)
    args = parser.parse_args()
    name = engine or args.name
    library = f"libplenty_{name}_v1.so"
    args.target_dir = args.target_dir or ROOT / "target" / f"{name}-engine"
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    version = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(line.removeprefix("host: ") for line in version.splitlines() if line.startswith("host: "))
    target = args.target or host
    if target not in ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"):
        parser.error(f"unsupported engine target: {target}")
    cpu, cflags = (("x86-64", "-march=x86-64 -mtune=generic") if target.startswith("x86_64")
                   else ("generic", "-march=armv8-a -mtune=generic"))
    linker_key = "CARGO_TARGET_" + target.upper().replace("-", "_") + "_LINKER"
    default_linker = "cc" if target == host else target.replace("-unknown", "") + "-gcc"
    linker = os.environ.get(linker_key, default_linker)
    env = {key: value for key, value in os.environ.items()
           if not (key.startswith(("CFLAGS", "CXXFLAGS", "TARGET_CFLAGS", "TARGET_CXXFLAGS")))}
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    env.update(CARGO_TARGET_DIR=str(args.target_dir.resolve()),
               **{linker_key: linker},
               RUSTFLAGS=f"-C target-cpu={cpu} -C link-arg=-Wl,-soname,{library}",
               CFLAGS=cflags,
               CXXFLAGS=cflags)
    subprocess.run(["cargo", "build", "--manifest-path", str(ROOT / f"plenty-{name}" / "Cargo.toml"),
                    "--locked", "--release", "--target", target, "--jobs", str(args.jobs)], env=env, check=True, cwd=ROOT)
    if args.validate:
        manifest = str(ROOT / f"plenty-{name}" / "Cargo.toml")
        subprocess.run(["cargo", "fmt", "--manifest-path", manifest, "--", "--check"], env=env, check=True, cwd=ROOT)
        common = ["--manifest-path", manifest, "--locked", "--release", "--target", target, "--jobs", str(args.jobs)]
        subprocess.run(["cargo", "test", *common], env=env, check=True, cwd=ROOT)
        subprocess.run(["cargo", "clippy", *common, "--all-targets", "--", "-D", "warnings"], env=env, check=True, cwd=ROOT)
    args.out.mkdir(parents=True, exist_ok=True)
    source = args.target_dir.resolve() / target / "release" / library
    destination = args.out.resolve() / library
    if source != destination:
        temporary = destination.with_suffix(f".tmp-{os.getpid()}")
        shutil.copy2(source, temporary)
        temporary.replace(destination)
    print(f"Installed baseline {target} {name} engine: {destination}")
    print(f"The compiler discovers it beside its executable, or via PLENTY_{name.upper()}_ENGINE.")


if __name__ == "__main__":
    main()
