#!/usr/bin/env python3
"""Write a reproducible .tar.gz or .zip of one staged release directory.

Usage: archive.py STAGE NAME OUT

Archives STAGE/NAME as OUT, choosing the format from OUT's suffix. Entries are
sorted, owned by 0:0 (tar), dated SOURCE_DATE_EPOCH (default 0), and given
mode 0755 for directories and files under bin/, 0644 otherwise, so the result
does not depend on the build host's tar, umask, or file system. Used for the
macOS and Windows archives; Linux archives keep GNU tar in package.sh.
"""
import gzip
import os
from pathlib import Path
import sys
import tarfile
import time
import zipfile


def entries(stage, name):
    root = stage / name
    found = [root]
    for directory, dirs, files in os.walk(root):
        dirs.sort()
        base = Path(directory)
        found.extend(base / d for d in dirs)
        found.extend(base / f for f in sorted(files))
    for path in sorted(found, key=lambda p: p.relative_to(stage).as_posix()):
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise SystemExit(f"archive.py: unsupported entry {path}")
        relative = path.relative_to(stage).as_posix()
        executable = path.is_dir() or relative.startswith(f"{name}/bin/")
        yield path, relative, 0o755 if executable else 0o644


def write_tar(stage, name, out, epoch):
    with open(out, "wb") as raw, \
            gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0) as packed, \
            tarfile.open(fileobj=packed, mode="w", format=tarfile.GNU_FORMAT) as archive:
        for path, relative, mode in entries(stage, name):
            info = tarfile.TarInfo(relative)
            info.mode, info.mtime = mode, epoch
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            if path.is_dir():
                info.type = tarfile.DIRTYPE
                archive.addfile(info)
            else:
                info.size = path.stat().st_size
                with open(path, "rb") as data:
                    archive.addfile(info, data)


def write_zip(stage, name, out, epoch):
    # ZIP timestamps are local DOS times from 1980; store the epoch as UTC.
    stamp = time.gmtime(max(epoch, 315532800))[:6]
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path, relative, mode in entries(stage, name):
            directory = path.is_dir()
            info = zipfile.ZipInfo(relative + "/" if directory else relative, stamp)
            info.create_system = 3  # Unix, so external_attr carries the mode
            info.external_attr = ((0o040000 if directory else 0o100000) | mode) << 16
            if directory:
                info.external_attr |= 0x10
                archive.writestr(info, b"")
            else:
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, path.read_bytes(), compresslevel=9)


def main(argv):
    if len(argv) != 3:
        raise SystemExit("usage: archive.py STAGE NAME OUT")
    stage, name, out = Path(argv[0]), argv[1], Path(argv[2])
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH") or 0)
    if out.name.endswith(".tar.gz"):
        write_tar(stage, name, out, epoch)
    elif out.suffix == ".zip":
        write_zip(stage, name, out, epoch)
    else:
        raise SystemExit(f"archive.py: unsupported archive type {out.name}")


if __name__ == "__main__":
    main(sys.argv[1:])
