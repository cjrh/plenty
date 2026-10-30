#!/usr/bin/env python3
"""Write the third-party notices shipped in a Plenty release archive.

The compiler executable links its normal Rust dependencies and embeds the
runtime archives, which contain `plenty-runtime`'s dependencies, the Rust
standard library, vendored SQLite amalgamation and Brotli codec, and vendored
sources (VENDORED_SOURCES). It also embeds the standard library modules, some
derived from third-party code or data (EMBEDDED). The optional dataframe
component uses its own workspace/lockfile and has neither vendored library.
A `vendor/` directory that no notice covers fails the compiler build. Build scripts,
procedural macros, and dev-dependencies do not ship and are not listed.

Every listed crate must provide its license text in its source package or
have a reviewed fallback below. A crate without one fails the build, so a
new dependency cannot ship without its notice being reviewed.

Usage: third_party_notices.py --repo DIR --out FILE [--component compiler|dataframe] [--list]
"""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

TARGET = "x86_64-unknown-linux-gnu"
# ring 0.17.14 ships for std.tls and advanced std.crypto. Its existing
# vendored source and complete notices are covered through Cargo metadata.
ROOTS = ("plenty", "plenty-runtime")
# BLAKE3 1.8.7 and its arrayvec, constant_time_eq and cpufeatures dependencies
# are covered by Cargo metadata, including their unmodified vendored copies.
LICENSE_PREFIXES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "UNLICENSE")
REPO = Path(__file__).resolve().parents[2]
LICENSE_DIR = Path(__file__).resolve().parent / "licenses"

# The published crate omits the repository license. Pin the fallback to the
# version and declared license; upgrades must be reviewed again. The commit
# comes from the crate's .cargo_vcs_info.json.
FALLBACK_LICENSE_TEXT = {
    ("yaxpeax-arm", "0.5.0", "0BSD"): (
        "yaxpeax-arm-0.5.0-LICENSE.txt",
        "https://github.com/iximeow/yaxpeax-arm/blob/"
        "a2723ae2043777472dd373feef4b9c40d52569bb/Cargo.toml",
    ),
    ("equator", "0.2.2", "MIT"): (
        "equator-0.2.2-LICENSE.txt",
        "https://raw.githubusercontent.com/sarah-ek/equator/9d4107bdd6a75598aed4a60fe46200c9e9f4e653/LICENSE",
    ),
    ("faer-traits", "0.24.0", "MIT"): (
        "faer-traits-0.24.0-LICENSE.txt",
        "https://raw.githubusercontent.com/sarah-quinones/faer-rs/8dfcceee8eb3d81231366acc9d3f157ac5e7057a/LICENSE",
    ),
    ("nano-gemm", "0.2.2", "MIT"): (
        "nano-gemm-0.2.2-LICENSE.txt",
        "https://raw.githubusercontent.com/sarah-ek/nano-gemm/806b012a6f5ee0615042bf64675180fb97ff2ccc/LICENSE",
    ),
    ("pulp-wasm-simd-flag", "0.1.1", "MIT"): (
        "pulp-wasm-simd-flag-0.1.1-LICENSE.txt",
        "https://raw.githubusercontent.com/sarah-quinones/pulp/5eb07fd7b68edf0a5e19f71737d315f72a510295/LICENSE",
    ),
    ("polars-parquet-format", "0.1.0", "MIT AND Apache-2.0"): (
        "polars-parquet-format-0.1.0-LICENSE.txt",
        "https://github.com/jorgecarleitao/parquet-format-safe/tree/"
        "b499e46b1d6ce29d6b971edf5f29cc2e6048bb34",
    ),
    ("iced-x86", "1.21.0", "MIT"): (
        "iced-x86-1.21.0-LICENSE.txt",
        "https://github.com/icedland/iced/blob/"
        "c50f29b7bc305696895c075f3fc7719751426b12/LICENSE.txt",
    ),
}
for _name in ("nano-gemm-core", "nano-gemm-f32", "nano-gemm-f64", "nano-gemm-c32", "nano-gemm-c64"):
    FALLBACK_LICENSE_TEXT[(_name, "0.2.1", "MIT")] = (
        "nano-gemm-0.2.2-LICENSE.txt",
        "https://raw.githubusercontent.com/sarah-ek/nano-gemm/d5f1cd77f0af31fb90aa0d128a76e2366c5dabc7/LICENSE",
    )

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

# Vendored native libraries rendered above; recorded so the vendor coverage
# check below knows their directories are covered.
NATIVE_DIRS = (
    "plenty-runtime/native/sqlite-3.54.0",
    "plenty-runtime/native/brotli-1.2.1",
)

# Source copied into the repository from a published package that is not a
# Cargo dependency. Each entry reproduces the license files kept beside the
# copy: (heading, license, use, source, directory, license files).
VENDORED_SOURCES = (
    (
        "unicode-normalization 0.1.25 (std.unicode normalization)",
        "MIT OR Apache-2.0",
        "plenty-runtime/src/unicode/vendor contains unmodified lookups.rs, "
        "normalize.rs, perfect_hash.rs and tables.rs from this crates.io release.",
        "https://github.com/unicode-rs/unicode-normalization/tree/"
        "5a69b3bafb625caccdec7871a42bed0d9a6604d1",
        "plenty-runtime/src/unicode/vendor",
        ("COPYRIGHT", "LICENSE-APACHE", "LICENSE-MIT"),
    ),
)


def vendored_source_notices():
    out = []
    for heading, license_id, use, source, directory, names in VENDORED_SOURCES:
        out.extend([f"## {heading}\n", f"License: {license_id}", f"Source: {source}", "", use, ""])
        for name in names:
            text = (REPO / directory / name).read_text(encoding="utf-8").rstrip()
            out.extend([f"### {name}\n", "```text", text, "```\n"])
    return "\n".join(out)


def uncovered_vendor_dirs(repo, packages):
    """Vendored directories that no notice covers.

    A directory named `vendor`, or a subdirectory of `plenty-runtime/native`,
    is covered when it is listed in NATIVE_DIRS or VENDORED_SOURCES. A child of
    a `vendor` directory is also covered when it is named `NAME-VERSION` after
    a package in `packages`, whose notice comes from Cargo metadata.
    """
    covered = set(NATIVE_DIRS) | {entry[4] for entry in VENDORED_SOURCES}
    crates = {f"{p['name']}-{p['version']}" for p in packages}
    uncovered = []
    for current, dirs, _ in os.walk(repo):
        dirs[:] = sorted(d for d in dirs if not d.startswith(".") and d != "target")
        if Path(current) == Path(repo):
            # Independent engines have separate dependency graphs and notices;
            # none of their sources ships in the compiler archive.
            dirs[:] = [d for d in dirs if d not in ("plenty-dataframe", "plenty-linalg")]
        relative = Path(current).relative_to(repo).as_posix()
        if relative == "plenty-runtime/native":
            uncovered.extend(f"{relative}/{d}" for d in dirs if f"{relative}/{d}" not in covered)
            dirs[:] = []
        elif Path(current).name == "vendor":
            if relative not in covered:
                if any(f.is_file() for f in Path(current).iterdir()):
                    uncovered.append(relative)
                uncovered.extend(f"{relative}/{d}" for d in dirs
                                 if d not in crates and f"{relative}/{d}" not in covered)
            dirs[:] = []
    return uncovered


# Code and data the compiler embeds that come from neither a Cargo package nor
# a vendored directory's own license files: std modules are compiled into the
# compiler executable, and runtime tables can derive from external data.
# Each entry names a reviewed license text in LICENSE_DIR; update the entry and
# the text together when the upstream version changes.
EMBEDDED = (
    (
        "commonmark.js 0.31.2 (std.markdown parser and renderer)",
        "BSD-2-Clause",
        "stdlib/std/markdown.plenty follows the commonmark.js parsing and "
        "rendering algorithms.",
        "https://github.com/commonmark/commonmark.js/blob/"
        "cb2c2303d3550ec6ef28ceb2841f148e8761eebf/LICENSE",
        "commonmark.js-0.31.2-LICENSE.txt",
    ),
    (
        "Unicode Character Database 16.0.0 (std.markdown punctuation table)",
        "Unicode-3.0",
        "stdlib/std/markdown.plenty contains punctuation ranges derived from "
        "the Unicode Character Database by scripts/gen_markdown_tables.py.",
        "https://www.unicode.org/license.txt",
        "unicode-license-v3.txt",
    ),
    (
        "Unicode Character Database 17.0.0 (std.unicode normalization tables)",
        "Unicode-3.0",
        "plenty-runtime/src/unicode/vendor/tables.rs contains decomposition, "
        "composition and canonical combining class tables that unicode-normalization "
        "0.1.25 generated from the Unicode Character Database.",
        "https://www.unicode.org/license.txt",
        "unicode-license-v3.txt",
    ),
    (
        "WHATWG HTML Standard named character references (std.markdown entity table)",
        "BSD-3-Clause",
        "stdlib/std/markdown.plenty contains the HTML named character "
        "references, generated by scripts/gen_markdown_tables.py. The Standard "
        "is CC-BY-4.0; portions incorporated into source code are BSD-3-Clause.",
        "https://html.spec.whatwg.org/multipage/named-characters.html",
        "whatwg-html-BSD-3-Clause.txt",
    ),
    (
        "similar 3.2.1 (std.diff algorithms)",
        "Apache-2.0",
        "stdlib/std/diff.plenty follows the similar crate's linear-space Myers "
        "and patience diff algorithms.",
        "https://github.com/mitsuhiko/similar/blob/"
        "71d4831349a61efa5c9fe521a344d8b335844d3a/LICENSE",
        "similar-3.2.1-LICENSE.txt",
    ),
)


def embedded_notices():
    out = []
    for heading, license_id, use, source, name in EMBEDDED:
        text = (LICENSE_DIR / name).read_text(encoding="utf-8").rstrip()
        out.extend([f"## {heading}\n", f"License: {license_id}", f"Source: {source}", "", use, ""])
        out.extend([f"### {name}\n", "```text", text, "```\n"])
    return "\n".join(out)


# Packages published without a license file, mapped to a package from the
# same upstream repository whose file carries their declared license text.
# Review each addition: the declared license must match that file.
SHARED_LICENSE_TEXT = {
    "cranelift-assembler-x64": "cranelift-codegen",
    "cranelift-bitset": "cranelift-codegen",
    "wasmtime-internal-core": "cranelift-codegen",
}
PINNED_SHARED_LICENSE_TEXT = {
    ("alloc-stdlib", "0.2.4", "BSD-3-Clause"): "alloc-no-stdlib",
}

RUST_STD = """\
## Rust standard library

This component contains the Rust standard library from the toolchain that
built this release. Its copyright and license
notices, including those of the crates it depends on, are in
`licenses/rust/COPYRIGHT-library.html`; the license texts it names are in
`licenses/rust/`.
"""


def shipped_packages(metadata, roots=ROOTS):
    """Packages reachable from ROOTS through normal dependencies only."""
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root_ids = [p["id"] for p in metadata["packages"] if p["name"] in roots and p["source"] is None]
    if len(root_ids) != len(roots):
        raise SystemExit(f"expected workspace packages {roots}, found {len(root_ids)}")
    seen, stack = set(), list(root_ids)
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


def license_files(package, include_nested=False):
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
    if include_nested:
        # Engine crates also vendor implementations with their own notices,
        # e.g. parquet-format's src/thrift/varint/LICENSE. Preserve those even
        # when the package needs a top-level license fallback as well.
        found.extend(f for f in sorted(directory.rglob("*"))
                     if f.is_file() and f.parent != directory
                     and f.name.upper().startswith(LICENSE_PREFIXES) and f not in found)
    return found


def render(packages, component="compiler"):
    out = [
        "# Third-party notices\n",
        "Plenty release archives contain the following third-party software.",
        "Each entry gives the package, its declared license, and the license",
        "and notice files from its source package or a reviewed upstream source.\n",
        VENDORED if component == "compiler" else "",
        ("## Brotli 1.2.1 (vendored reference C codec)\n\nLicense: MIT\n\n"
         + (REPO / "plenty-runtime/native/brotli-1.2.1/LICENSE").read_text()
         if component == "compiler" else ""),
        vendored_source_notices() if component == "compiler" else "",
        embedded_notices() if component == "compiler" else "",
        RUST_STD,
    ]
    missing = []
    nested = component != "compiler"
    names = {p["name"] for p in packages if license_files(p)}
    for package in packages:
        primary_files = license_files(package)
        files = license_files(package, nested)
        shared = PINNED_SHARED_LICENSE_TEXT.get(
            (package["name"], package["version"], package.get("license")),
            SHARED_LICENSE_TEXT.get(package["name"]),
        )
        if not primary_files and not (shared and shared in names):
            missing.append(f"{package['name']} {package['version']}")
            continue
        out.append(f"## {package['name']} {package['version']}\n")
        out.append(f"License: {package.get('license') or 'see files below'}")
        if package.get("repository"):
            out.append(f"Source: {package['repository']}")
        if (package.get("source") or "").startswith("git+"):
            out.append(f"Resolved Git source: {package['source']}")
        if not primary_files:
            out.append(f"License text: as reproduced under {shared}, from the same repository.")
        out.append("")
        for f in files:
            if f.parent == LICENSE_DIR:
                fallback = FALLBACK_LICENSE_TEXT[
                    (package["name"], package["version"], package.get("license"))
                ]
                out.append(f"License text source: {fallback[1]}\n")
            text = f.read_text(encoding="utf-8", errors="replace").rstrip()
            label = f.name if f.parent == LICENSE_DIR else str(f.relative_to(Path(package["manifest_path"]).parent))
            out.append(f"### {label}\n")
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
    parser.add_argument("--component", choices=("compiler", "dataframe", "linalg"), default="compiler")
    args = parser.parse_args()
    manifest = args.repo / (f"plenty-{args.component}/Cargo.toml" if args.component != "compiler" else "Cargo.toml")
    metadata = json.loads(
        subprocess.run(
            [
                "cargo", "metadata", "--locked", "--format-version", "1",
                "--filter-platform", TARGET,
                "--manifest-path", str(manifest),
            ],
            check=True,
            capture_output=True,
            env={**os.environ, "CARGO_TERM_COLOR": "never"},
        ).stdout
    )
    packages = shipped_packages(metadata, (f"plenty-{args.component}-engine",) if args.component != "compiler" else ROOTS)
    if args.component == "compiler":
        uncovered = uncovered_vendor_dirs(args.repo, packages)
        if uncovered:
            raise SystemExit("vendored directories without a notice: " + ", ".join(uncovered))
    if args.list:
        for p in packages:
            print(p["name"], p["version"], p.get("license"), len(license_files(p, args.component != "compiler")), file=sys.stderr)
    args.out.write_text(render(packages, args.component), encoding="utf-8")


if __name__ == "__main__":
    main()
