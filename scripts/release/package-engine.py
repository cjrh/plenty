#!/usr/bin/env python3
"""Audit an engine for a portable release archive or explicit local installation."""

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
TARGET = "x86_64-unknown-linux-gnu"


def output(*args):
    return subprocess.check_output(args, text=True).strip()


def check_exports(engine, name="dataframe"):
    """Reject a stale engine even when its SONAME has the same ABI generation."""
    prefix = {"dataframe": "pldf", "linalg": "plla"}[name]
    required = set(re.findall(rf'= "({prefix}_v1_\w+)"', (REPO / f"stdlib/std/{name}.plentyi").read_text()))
    if not required:
        raise SystemExit(f"no {name} facade exports found")
    defined = {line.split()[-1] for line in output("nm", "-D", "--defined-only", str(engine)).splitlines()}
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
    parser.add_argument("--engine", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--rust-sysroot", type=Path)
    parser.add_argument("--local-install", action="store_true",
                        help="install NAME-notices beside an engine in --out-dir; no portable archive")
    args = parser.parse_args()
    component = component or args.name
    library = f"libplenty_{component}_v1.so"
    engine = args.engine.resolve(strict=True)
    if args.local_install and engine != (args.out_dir / library).resolve():
        parser.error("--local-install requires --engine to be the library in --out-dir")
    audit_mode = "--local-engine" if args.local_install else "--engine"
    subprocess.run([str(HERE / "check-elf.sh"), audit_mode, library, str(engine)], check=True)
    check_exports(engine, component)
    version = output(str(HERE / "check-version.sh"), "--print")
    name = f"plenty-{component}-{version}-{TARGET}"
    sysroot = args.rust_sysroot or Path(output("rustc", "--print", "sysroot"))
    rust_docs = sysroot / "share/doc/rust"
    rust_licenses = sorted((rust_docs / "licenses").glob("*.txt"))
    if not rust_licenses:
        raise SystemExit(f"no Rust standard-library license texts in {rust_docs / 'licenses'}")
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH") or output("git", "-C", str(REPO), "log", "-1", "--format=%ct"))
    args.out_dir.mkdir(parents=True, exist_ok=True)
    archive = args.out_dir / f"{name}.tar.gz"
    with tempfile.TemporaryDirectory(prefix=f".plenty-{component}-package-", dir=args.out_dir) as directory:
        root = Path(directory) / name
        (root / "lib").mkdir(parents=True)
        (root / "licenses/rust").mkdir(parents=True)
        shutil.copyfile(engine, root / "lib" / library)
        for filename in ("LICENSE-MIT", "LICENSE-APACHE"):
            shutil.copyfile(REPO / filename, root / filename)
        for source in [rust_docs / "COPYRIGHT-library.html", *rust_licenses]:
            shutil.copyfile(source, root / "licenses/rust" / source.name)
        readme = (HERE / f"README.{component}.md").read_text().replace("@VERSION@", version).replace("@TARGET@", TARGET)
        (root / "README.md").write_text(readme)
        subprocess.run(["python3", "-B", str(HERE / "third_party_notices.py"), "--repo", str(REPO),
                        "--component", component, "--out", str(root / "THIRD-PARTY-NOTICES.md")], check=True)
        manifest = {
            "format": 1, "plenty_version": version, "target": TARGET,
            "abi_generation": 1, "library": f"lib/{library}",
            "minimum_glibc": minimum_glibc(engine) if args.local_install else "2.28",
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
        if args.local_install:
            shutil.rmtree(root / "lib")
            destination = args.out_dir.resolve() / f"{component}-notices"
            install_notices(root, destination)
            print(f"Installed local engine notices: {destination}")
            return
        with archive.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode="w", format=tarfile.GNU_FORMAT) as tar:
                for path in [root, *sorted(root.rglob("*"))]:
                    info = tar.gettarinfo(str(path), arcname=str(path.relative_to(root.parent)))
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    info.mtime = epoch
                    info.mode = 0o755 if path.is_dir() else 0o644
                    if path.is_file():
                        with path.open("rb") as data:
                            tar.addfile(info, data)
                    else:
                        tar.addfile(info)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
    print(archive)


if __name__ == "__main__":
    main()
