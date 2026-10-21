#!/usr/bin/env python3
"""Write the third-party notices shipped in a Plenty release archive.

The compiler executable links its normal Rust dependencies and embeds the
runtime archives, which contain `plenty-runtime`'s dependencies, the Rust
standard library, and the vendored SQLite amalgamation. Build scripts,
procedural macros, and dev-dependencies do not ship and are not listed.

Every listed crate must provide its license text in its source package or
have a reviewed fallback below. A crate without one fails the build, so a
new dependency cannot ship without its notice being reviewed.

Usage: third_party_notices.py --repo DIR --out FILE [--list]
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

TARGET = "x86_64-unknown-linux-gnu"
ROOTS = ("plenty", "plenty-runtime")
LICENSE_PREFIXES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "UNLICENSE")
LICENSE_DIR = Path(__file__).resolve().parent / "licenses"

# The published crate omits the repository license. Pin the fallback to the
# version and declared license; upgrades must be reviewed again. The commit
# comes from the crate's .cargo_vcs_info.json.
FALLBACK_LICENSE_TEXT = {
    ("iced-x86", "1.21.0", "MIT"): (
        "iced-x86-1.21.0-LICENSE.txt",
        "https://github.com/icedland/iced/blob/"
        "c50f29b7bc305696895c075f3fc7719751426b12/LICENSE.txt",
    ),
}

# Vendored C code that is not a Cargo package; see build.rs.
VENDORED = """\
## SQLite 3.54.0 (vendored C amalgamation, plenty-runtime/native/sqlite-3.54.0)

License: public domain

The author disclaims copyright to this source code. In place of a legal
notice, here is a blessing:

    May you do good and not evil.
    May you find forgiveness for yourself and forgive others.
    May you share freely, never taking more than you give.

See https://sqlite.org/copyright.html.
"""

# Packages published without a license file, mapped to a package from the
# same upstream repository whose file carries their declared license text.
# Review each addition: the declared license must match that file.
SHARED_LICENSE_TEXT = {
    "cranelift-assembler-x64": "cranelift-codegen",
    "cranelift-bitset": "cranelift-codegen",
    "wasmtime-internal-core": "cranelift-codegen",
}

RUST_STD = """\
## Rust standard library

The compiler and the runtime archives it embeds contain the Rust standard
library from the toolchain that built this release. Its copyright and license
notices, including those of the crates it depends on, are in
`licenses/rust/COPYRIGHT-library.html`; the license texts it names are in
`licenses/rust/`.
"""


def shipped_packages(metadata):
    """Packages reachable from ROOTS through normal dependencies only."""
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    roots = [p["id"] for p in metadata["packages"] if p["name"] in ROOTS and p["source"] is None]
    if len(roots) != len(ROOTS):
        raise SystemExit(f"expected workspace packages {ROOTS}, found {len(roots)}")
    seen, stack = set(), list(roots)
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        for dep in nodes[current]["deps"]:
            if not any(k["kind"] is None for k in dep["dep_kinds"]):
                continue
            target = packages[dep["pkg"]]
            if any("proc-macro" in t["kind"] for t in target["targets"]):
                continue
            stack.append(dep["pkg"])
    return sorted(
        (packages[i] for i in seen if packages[i]["source"] is not None),
        key=lambda p: (p["name"], p["version"]),
    )


def license_files(package):
    directory = Path(package["manifest_path"]).parent
    found = [
        f
        for f in sorted(directory.iterdir())
        if f.is_file() and f.name.upper().startswith(LICENSE_PREFIXES)
    ]
    if package.get("license_file"):
        named = directory / package["license_file"]
        if named.is_file() and named not in found:
            found.append(named)
    if not found:
        fallback = FALLBACK_LICENSE_TEXT.get(
            (package["name"], package["version"], package.get("license"))
        )
        if fallback:
            found.append(LICENSE_DIR / fallback[0])
    return found


def render(packages):
    out = [
        "# Third-party notices\n",
        "Plenty release archives contain the following third-party software.",
        "Each entry gives the package, its declared license, and the license",
        "and notice files from its source package or a reviewed upstream source.\n",
        VENDORED,
        RUST_STD,
    ]
    missing = []
    names = {p["name"] for p in packages if license_files(p)}
    for package in packages:
        files = license_files(package)
        shared = SHARED_LICENSE_TEXT.get(package["name"])
        if not files and not (shared and shared in names):
            missing.append(f"{package['name']} {package['version']}")
            continue
        out.append(f"## {package['name']} {package['version']}\n")
        out.append(f"License: {package.get('license') or 'see files below'}")
        if package.get("repository"):
            out.append(f"Source: {package['repository']}")
        if not files:
            out.append(f"License text: as reproduced under {shared}, from the same repository.")
        out.append("")
        for f in files:
            if f.parent == LICENSE_DIR:
                fallback = FALLBACK_LICENSE_TEXT[
                    (package["name"], package["version"], package.get("license"))
                ]
                out.append(f"License text source: {fallback[1]}\n")
            text = f.read_text(encoding="utf-8", errors="replace").rstrip()
            out.append(f"### {f.name}\n")
            out.append("```text")
            out.append(text)
            out.append("```\n")
    if missing:
        raise SystemExit("no license file in shipped packages: " + ", ".join(missing))
    return "\n".join(out) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--repo", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--list", action="store_true", help="print name, version, license only")
    args = parser.parse_args()
    metadata = json.loads(
        subprocess.run(
            [
                "cargo", "metadata", "--locked", "--format-version", "1",
                "--filter-platform", TARGET,
                "--manifest-path", str(args.repo / "Cargo.toml"),
            ],
            check=True,
            capture_output=True,
            env={**os.environ, "CARGO_TERM_COLOR": "never"},
        ).stdout
    )
    packages = shipped_packages(metadata)
    if args.list:
        for p in packages:
            print(p["name"], p["version"], p.get("license"), len(license_files(p)), file=sys.stderr)
    args.out.write_text(render(packages), encoding="utf-8")


if __name__ == "__main__":
    main()
