#!/usr/bin/env python3
"""Compare hand-lowered candidates with real Plenty and Rust compilers."""
import argparse
import json
import os
from pathlib import Path
import resource
import subprocess

ROOT = Path(__file__).resolve().parent


def command(args, *, ok=True, env=None):
    result = subprocess.run(args, cwd=ROOT, text=True, capture_output=True, env=env)
    if ok and result.returncode:
        raise RuntimeError(f"{args}:\n{result.stdout}\n{result.stderr}")
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--plenty", type=Path, required=True)
    args = parser.parse_args()
    plenty = args.plenty.resolve()
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    output = ROOT / "target" / "observations"
    output.mkdir(parents=True, exist_ok=True)
    report = {
        "rustc": command(["rustc", "-Vv"]).stdout.strip(),
        "plenty": command([str(plenty), "--version"]).stdout.strip(),
        "note": "Hand-lowered probes, not compiler-generated Rust or a speed ranking.",
        "checks": [],
    }
    for fixture, code in [("reject-index-loans", "E0499"), ("reject-moved-owner", "E0382")]:
        result = command(
            ["rustc", "--edition=2024", f"{fixture}.rs", "-o", str(output / fixture)],
            ok=False,
        )
        assert result.returncode and code in result.stderr, result.stderr
        report["checks"].append(f"{fixture}: {code}")

    for profile in ["debug", "release"]:
        flags = ["--release"] if profile == "release" else []
        command(["cargo", "test", "--offline", "--locked", *flags])
        command(["cargo", "build", "--offline", "--locked", *flags])
        rust = ROOT / "target" / profile / "plenty-rust-source-spike"
        for name in ["semantics", "overflow"]:
            native = output / f"{name}-{profile}"
            command([str(plenty), *flags, "--compile", str(ROOT / f"{name}.plenty"), "-o", str(native)])
            left = command([str(native)], ok=False)
            right = command([str(rust), name], ok=False)
            if name == "semantics":
                assert left.returncode == right.returncode == 0, (left.stderr, right.stderr)
                assert left.stdout == right.stdout == "33\n2\n1\n3\n4\n", (left.stdout, right.stdout)
            else:
                assert left.returncode != 0 and right.returncode != 0
                assert not left.stdout and not right.stdout, (left.stdout, right.stdout)
                assert "overflow" in left.stderr and "overflow" in right.stderr
            report["checks"].append(f"{profile} {name}: matched output and exit category")
        panic = command([str(rust), "panic"], ok=False)
        assert panic.returncode != 0 and panic.stdout == "99\n", panic
        report["checks"].append(f"{profile} default Rust panic: destructor ran (mismatch)")

    abort_target = ROOT / "target" / "abort-graph"
    env = dict(os.environ, CARGO_TARGET_DIR=str(abort_target), RUSTFLAGS="-C panic=abort")
    command(["cargo", "build", "--offline", "--locked"], env=env)
    panic = command([str(abort_target / "debug" / "plenty-rust-source-spike"), "panic"], ok=False)
    assert panic.returncode != 0 and not panic.stdout, panic
    report["checks"].append("panic=abort build: destructor did not run")
    report["checks"].append("Rust host API / fallible reservation / split loans: 3 tests x 2 profiles")
    (output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
