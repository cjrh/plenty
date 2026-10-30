#!/usr/bin/env python3
"""Build and execute the isolated Linux ARM backend probe under QEMU.

First provision (downloads a pinned GCC/glibc toolchain into a fresh directory):
  python3 scripts/aarch64_probe.py --provision /tmp/plenty-arm-toolchain
Then run, with Cargo's target directory on a filesystem with sufficient space:
  CARGO_TARGET_DIR=/path/to/target python3 scripts/aarch64_probe.py \
      --toolchain /tmp/plenty-arm-toolchain --out /tmp/plenty-arm-probe

This wraps only the probe executable; Cargo runners do not automatically wrap
subprocesses spawned by Plenty integration tests. Run those in an ARM guest.
"""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

URL = ("https://toolchains.bootlin.com/downloads/releases/toolchains/aarch64/"
       "tarballs/aarch64--glibc--stable-2025.08-1.tar.xz")
SHA256 = "dfb47eee874eef9e8a7fc042eee4e0a183f444b6bcde6a82fef8f009918389c9"
PREFIX = "aarch64-buildroot-linux-gnu"


def provision(destination):
    if destination.exists():
        raise SystemExit(f"Refusing to replace existing toolchain: {destination}")
    with tempfile.TemporaryDirectory(prefix="plenty-arm-download-") as temporary:
        archive = Path(temporary) / "toolchain.tar.xz"
        urllib.request.urlretrieve(URL, archive)
        with archive.open("rb") as downloaded:
            if hashlib.file_digest(downloaded, "sha256").hexdigest() != SHA256:
                raise SystemExit("Toolchain SHA256 mismatch")
        with tarfile.open(archive) as package:
            # The pinned toolchain contains symlinks within its sysroot.
            package.extractall(Path(temporary) / "unpacked", filter="tar")
        roots = list((Path(temporary) / "unpacked").iterdir())
        if len(roots) != 1:
            raise SystemExit("Expected one toolchain archive root")
        shutil.move(roots[0], destination)


def run(command, **kwargs):
    print("+", " ".join(map(str, command)), flush=True)
    subprocess.run(command, check=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provision", type=Path)
    parser.add_argument("--toolchain", type=Path)
    parser.add_argument("--out", type=Path, default=Path("target/aarch64-probe"))
    args = parser.parse_args()
    if args.provision:
        provision(args.provision.resolve())
        return
    if not args.toolchain:
        parser.error("--toolchain is required for execution")
    toolchain = args.toolchain.resolve()
    cc = toolchain / "bin" / f"{PREFIX}-gcc"
    sysroot = toolchain / PREFIX / "sysroot"
    root = Path(__file__).resolve().parent / "aarch64-probe"
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.setdefault("CARGO_BUILD_JOBS", "2")
    run([cc, "--version"])
    run(["qemu-aarch64", "--version"])
    run(["cargo", "run", "--locked", "--offline", "--manifest-path", root / "Cargo.toml",
         "--", out / "probe.o"], env=env)
    run([cc, "-O2", "-Wall", "-Werror", root / "driver.c", out / "probe.o", "-o", out / "probe"])
    run([toolchain / "bin" / f"{PREFIX}-readelf", "-h", "-r", out / "probe.o"])
    # SIGILL is expected in a child; prevent a core file from consuming disk.
    import resource
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    run(["qemu-aarch64", "-cpu", "cortex-a53", "-L", sysroot, out / "probe"])


if __name__ == "__main__":
    main()
