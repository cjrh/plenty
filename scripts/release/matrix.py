#!/usr/bin/env python3
"""Emit native release build and smoke matrices from the pinned shell inputs."""
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
TARGETS = (("x86_64-unknown-linux-gnu", "ubuntu-24.04"),
           ("aarch64-unknown-linux-gnu", "ubuntu-24.04-arm"))


def matrices():
    builds, smokes = [], []
    for target, runner in TARGETS:
        result = subprocess.check_output(
            ["bash", "-c", '. "$1/toolchain.env"; . "$1/target.sh"; printf "%s\\n%s" "$BUILD_IMAGE" "$SMOKE_IMAGES"',
             "release-matrix", str(HERE)], env={**os.environ, "PLENTY_RELEASE_TARGET": target}, text=True)
        image, smoke = result.split("\n", 1)
        builds.append({"target": target, "runner": runner, "image": image})
        smokes.extend({"target": target, "runner": runner, "image": item} for item in smoke.split())
    return {"build_matrix": {"include": builds}, "smoke_matrix": {"include": smokes}}


if __name__ == "__main__":
    for key, value in matrices().items():
        print(key + "=" + json.dumps(value, separators=(",", ":")))
