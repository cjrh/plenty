#!/usr/bin/env python3
"""Audit and package the optional engine built from this release checkout."""

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
LIBRARY = "libplenty_dataframe_v1.so"


def output(*args):
    return subprocess.check_output(args, text=True).strip()


def check_exports(engine):
    """Reject a stale engine even when its SONAME has the same ABI generation."""
    required = set(re.findall(r'= "(pldf_v1_\w+)"', (REPO / "stdlib/std/dataframe.plentyi").read_text()))
    if not required:
        raise SystemExit("no dataframe facade exports found")
    defined = {line.split()[-1] for line in output("nm", "-D", "--defined-only", str(engine)).splitlines()}
    missing = required - defined
    if missing:
        raise SystemExit("engine does not match the release facade; missing exports: " + ", ".join(sorted(missing)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", type=Path, required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--rust-sysroot", type=Path)
    args = parser.parse_args()
    engine = args.engine.resolve(strict=True)
    subprocess.run([str(HERE / "check-elf.sh"), "--dataframe-engine", str(engine)], check=True)
    check_exports(engine)
    version = output(str(HERE / "check-version.sh"), "--print")
    name = f"plenty-dataframe-{version}-{TARGET}"
    sysroot = args.rust_sysroot or Path(output("rustc", "--print", "sysroot"))
    rust_docs = sysroot / "share/doc/rust"
    rust_licenses = sorted((rust_docs / "licenses").glob("*.txt"))
    if not rust_licenses:
        raise SystemExit(f"no Rust standard-library license texts in {rust_docs / 'licenses'}")
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH") or output("git", "-C", str(REPO), "log", "-1", "--format=%ct"))
    args.out_dir.mkdir(parents=True, exist_ok=True)
    archive = args.out_dir / f"{name}.tar.gz"
    with tempfile.TemporaryDirectory(prefix="plenty-dataframe-package-") as directory:
        root = Path(directory) / name
        (root / "lib").mkdir(parents=True)
        (root / "licenses/rust").mkdir(parents=True)
        shutil.copyfile(engine, root / "lib" / LIBRARY)
        for filename in ("LICENSE-MIT", "LICENSE-APACHE"):
            shutil.copyfile(REPO / filename, root / filename)
        for source in [rust_docs / "COPYRIGHT-library.html", *rust_licenses]:
            shutil.copyfile(source, root / "licenses/rust" / source.name)
        readme = (HERE / "README.dataframe.md").read_text().replace("@VERSION@", version).replace("@TARGET@", TARGET)
        (root / "README.md").write_text(readme)
        subprocess.run(["python3", "-B", str(HERE / "third_party_notices.py"), "--repo", str(REPO),
                        "--component", "dataframe", "--out", str(root / "THIRD-PARTY-NOTICES.md")], check=True)
        manifest = {
            "format": 1, "plenty_version": version, "target": TARGET,
            "abi_generation": 1, "library": f"lib/{LIBRARY}", "minimum_glibc": "2.28",
            "source_commit": output("git", "-C", str(REPO), "rev-parse", "HEAD"),
            "library_sha256": hashlib.sha256(engine.read_bytes()).hexdigest(),
        }
        (root / "engine.json").write_text(json.dumps(manifest, indent=2) + "\n")
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
