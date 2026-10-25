#!/usr/bin/env python3
"""Build the optional engine once, recording explicit compiler flags and cost."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", type=Path, required=True)
    parser.add_argument("--clean", action="store_true", help="require a new target directory; never delete one")
    args = parser.parse_args()
    target = args.target.resolve()
    if args.clean and target.exists():
        parser.error("--clean requires a target directory that does not exist")
    target.mkdir(parents=True, exist_ok=True)
    label = "clean-build" if args.clean else "incremental-build"
    # Override machine-local rustflags; v3 is an explicit spike deployment floor,
    # not runtime ISA dispatch or a change to Plenty's baseline target policy.
    flags = "-C target-cpu=x86-64-v3 -C link-arg=-fuse-ld=mold -C link-arg=-Wl,--thread-count=3"
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), RUSTFLAGS=flags,
               CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="cc")
    command = ["cargo", "build", "--locked", "--release", "--workspace", "-j", "3"]
    start = time.perf_counter()
    with (target / f"{label}.log").open("w") as log:
        result = subprocess.run(["/usr/bin/time", "-v", "-o", str(target / f"{label}.time"), *command],
                                cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    report = {"command": command, "rustflags": flags, "linker_driver": "cc",
              "clean_target": args.clean, "seconds": time.perf_counter() - start,
              "status": result.returncode,
              "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
              "time": (target / f"{label}.time").read_text()}
    (target / f"{label}.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
