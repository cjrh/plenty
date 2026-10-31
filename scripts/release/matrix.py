#!/usr/bin/env python3
"""Emit the release job matrices from the pinned shell inputs.

Usage: matrix.py [--select all|linux|native]

Linux targets build in a pinned glibc 2.28 image and smoke-test in clean
distribution containers. The experimental macOS and Windows targets build and
smoke-test natively on GitHub-hosted runners. A tag release always selects
`all`; a narrower selection is only for workflow_dispatch dry runs.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
LINUX = (("x86_64-unknown-linux-gnu", "ubuntu-24.04"),
         ("aarch64-unknown-linux-gnu", "ubuntu-24.04-arm"))
# Same runners as .github/workflows/portability.yml.
NATIVE = (("aarch64-apple-darwin", "macos-15"),
          ("x86_64-pc-windows-msvc", "windows-2025"))
SELECTIONS = {"all": LINUX + NATIVE, "linux": LINUX, "native": NATIVE}


def facts(target):
    """The target.sh (and, for Linux, toolchain.env image) facts for target."""
    script = ('. "$1/toolchain.env"; . "$1/target.sh"; '
              'printf "%s\\n%s\\n%s\\n%s" "$release_archive_ext" "$release_components" "$BUILD_IMAGE" "$SMOKE_IMAGES"')
    result = subprocess.check_output(["bash", "-c", script, "release-matrix", str(HERE)],
                                     env={**os.environ, "PLENTY_RELEASE_TARGET": target}, text=True)
    ext, components, image, smoke = result.split("\n", 3)
    return ext, components.split(), image, smoke.split()


def matrices(select="all"):
    chosen = {target for target, _ in SELECTIONS[select]}
    builds, smokes, native = [], [], []
    for target, runner in LINUX:
        if target not in chosen:
            continue
        _, _, image, smoke = facts(target)
        builds.append({"target": target, "runner": runner, "image": image})
        smokes.extend({"target": target, "runner": runner, "image": item} for item in smoke)
    for target, runner in NATIVE:
        if target not in chosen:
            continue
        ext, components, _, _ = facts(target)
        native.append({"target": target, "runner": runner, "ext": ext, "components": " ".join(components)})
    targets = [target for target, _ in LINUX + NATIVE if target in chosen]
    return {
        "build_matrix": {"include": builds},
        "smoke_matrix": {"include": smokes},
        "native_matrix": {"include": native},
        "linux": "true" if builds else "false",
        "native": "true" if native else "false",
        "targets": " ".join(targets),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--select", choices=SELECTIONS, default="all")
    args = parser.parse_args()
    for key, value in matrices(args.select).items():
        text = value if isinstance(value, str) else json.dumps(value, separators=(",", ":"))
        print(key + "=" + text)


if __name__ == "__main__":
    main()
