#!/usr/bin/env python3
"""Audit an engine and install it with its NAME-notices directory.

The release build installs both engines into the compiler archive's bin/,
where the compiler finds them and --dist finds their notices.
"""

import argparse
import hashlib
import importlib.util
import json
import os
import platform
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location("check_native", Path(__file__).with_name("check-native.py"))
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
TARGET = ("x86_64-pc-windows-msvc" if platform.system() == "Windows" and platform.machine().lower() in ("amd64", "x86_64")
          else "aarch64-apple-darwin" if platform.system() == "Darwin" and platform.machine() == "arm64"
          else {"x86_64": "x86_64-unknown-linux-gnu", "aarch64": "aarch64-unknown-linux-gnu"}.get(platform.machine()))


def output(*args):
    return subprocess.check_output(args, text=True).strip()


def check_exports(engine, name="dataframe", windows=False, macos=False):
    """Reject a stale engine even when its SONAME has the same ABI generation."""
    prefix = {"dataframe": "pldf", "linalg": "plla"}[name]
    required = set(re.findall(rf'= "({prefix}_v1_\w+)"', (REPO / f"stdlib/std/{name}.plentyi").read_text()))
    if not required:
        raise SystemExit(f"no {name} facade exports found")
    if macos:
        defined = {line.split()[-1].removeprefix("_") for line in output("nm", "-gU", str(engine)).splitlines()}
    else:
        defined = native.pe_exports(engine.read_bytes()) if windows else {line.split()[-1] for line in output("nm", "-D", "--defined-only", str(engine)).splitlines()}
    missing = required - defined
    if missing:
        raise SystemExit("engine does not match the release facade; missing exports: " + ", ".join(sorted(missing)))


def minimum_glibc(engine):
    """Record the library's actual symbol floor, not the build host's version."""
    versions = re.findall(r"\bGLIBC_(\d+(?:\.\d+)+)\b", output("readelf", "-V", str(engine)))
    if not versions:
        raise SystemExit("engine has no GLIBC version requirements")
    return max(versions, key=lambda version: tuple(map(int, version.split("."))))


def install_notices(root, destination):
    """Replace generated notices only after every license and manifest is ready."""
    if destination.is_symlink() or (destination.exists() and not destination.is_dir()):
        raise SystemExit(f"not a regular notices directory: {destination}")
    backup = root.parent / "old-notices"
    if destination.exists():
        destination.rename(backup)
    try:
        root.rename(destination)
    except OSError:
        if backup.exists():
            backup.rename(destination)
        raise


def main(component=None):
    parser = argparse.ArgumentParser(description=__doc__)
    if component is None:
        parser.add_argument("name", choices=("dataframe", "linalg"))
    parser.add_argument("--target", choices=("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "x86_64-pc-windows-msvc", "aarch64-apple-darwin"), default=os.environ.get("PLENTY_RELEASE_TARGET", TARGET))
    parser.add_argument("--engine", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True,
                        help="directory receiving the library and NAME-notices, such as a compiler's bin/")
    parser.add_argument("--rust-sysroot", type=Path)
    parser.add_argument("--local-install", action="store_true",
                        help="audit for this host, not the release baseline; --engine must already be in --out-dir")
    args = parser.parse_args()
    component = component or args.name
    target = args.target
    if target is None:
        parser.error("--target is required for an unsupported host")
    windows = target == "x86_64-pc-windows-msvc"
    macos = target == "aarch64-apple-darwin"
    if (windows or macos) and component != "dataframe":
        parser.error("Windows and macOS support the dataframe engine only")
    library = f"plenty_{component}_v1.dll" if windows else f"libplenty_{component}_v1.dylib" if macos else f"libplenty_{component}_v1.so"
    engine = args.engine.resolve(strict=True)
    if args.local_install and engine != (args.out_dir / library).resolve():
        parser.error("--local-install requires --engine to be the library in --out-dir")
    audit_mode = "--local-engine" if args.local_install else "--engine"
    if macos:
        problems, _ = native.check_macho(engine.read_bytes(), "11.0", library=library)
        if problems:
            raise SystemExit("; ".join(problems))
        subprocess.run(["codesign", "--verify", "--strict", str(engine)], check=True)
    elif windows:
        problems, _ = native.check_pe(engine.read_bytes(), dll=True)
        if problems:
            raise SystemExit("; ".join(problems))
    else:
        subprocess.run([str(HERE / "check-elf.sh"), audit_mode, library, str(engine)], check=True, env={**os.environ, "PLENTY_RELEASE_TARGET": target})
    check_exports(engine, component, windows, macos)
    version = output("bash", str(HERE / "check-version.sh"), "--print")
    sysroot = args.rust_sysroot or Path(output("rustc", "--print", "sysroot"))
    rust_docs = sysroot / "share/doc/rust"
    rust_licenses = sorted((rust_docs / "licenses").glob("*.txt"))
    if not rust_licenses:
        raise SystemExit(f"no Rust standard-library license texts in {rust_docs / 'licenses'}")
    args.out_dir.mkdir(parents=True, exist_ok=True)
    out_dir = args.out_dir.resolve()
    with tempfile.TemporaryDirectory(prefix=f".plenty-{component}-package-", dir=out_dir) as directory:
        root = Path(directory) / f"{component}-notices"
        (root / "licenses/rust").mkdir(parents=True)
        for filename in ("LICENSE-MIT", "LICENSE-APACHE"):
            shutil.copyfile(REPO / filename, root / filename)
        for source in [rust_docs / "COPYRIGHT-library.html", *rust_licenses]:
            shutil.copyfile(source, root / "licenses/rust" / source.name)
        subprocess.run([sys.executable, "-B", str(HERE / "third_party_notices.py"), "--repo", str(REPO),
                        "--target", target, "--component", component, "--out", str(root / "THIRD-PARTY-NOTICES.md")], check=True)
        manifest = {
            "format": 1, "plenty_version": version, "target": target,
            "abi_generation": 1, "library": f"lib/{library}",
            **({"minimum_windows": "10"} if windows else {"minimum_macos": "11.0"} if macos else {"minimum_glibc": minimum_glibc(engine) if args.local_install else "2.28"}),
            "source_commit": output("git", "-C", str(REPO), "rev-parse", "HEAD"),
            "library_sha256": hashlib.sha256(engine.read_bytes()).hexdigest(),
        }
        if args.local_install:
            manifest["build_kind"] = "local"
            manifest["source_dirty"] = bool(output("git", "-C", str(REPO), "status", "--porcelain", "--untracked-files=normal", "--", ".", ":(exclude)issues.db"))
            (root / "README.md").write_text(
                "# Local engine build\n\nBuilt for this host, not audited against the portable release baseline.\n"
                "Keep this notices directory beside the matching library for Plenty --dist.\n"
                "Rebuild after changing engine sources; source_commit is not an attestation.\n")
        (root / "engine.json").write_text(json.dumps(manifest, indent=2) + "\n")
        installed = out_dir / library
        if engine != installed.resolve():
            # Stage beside the destination so the final rename cannot cross filesystems.
            staged = Path(directory) / library
            shutil.copyfile(engine, staged)
            staged.chmod(0o644)
            staged.replace(installed)
        install_notices(root, out_dir / f"{component}-notices")
    print(f"Installed {component} engine and notices: {out_dir}")


if __name__ == "__main__":
    main()
