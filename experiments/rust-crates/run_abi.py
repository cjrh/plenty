#!/usr/bin/env python3
"""Run the separate nightly compiler-private ABI observation (no CLIF consumer)."""
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "target" / "probe-results"
TOOLCHAIN = "nightly-2026-10-01"


def run(args, **kwargs):
    result = subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True, **kwargs)
    return result.stdout


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    version = run(["rustc", f"+{TOOLCHAIN}", "-vV"])
    sysroot = run(["rustc", f"+{TOOLCHAIN}", "--print", "sysroot"]).strip()
    binary = OUT / "abi-helper"
    run(["rustc", f"+{TOOLCHAIN}", "abi-helper.rs", "--edition=2024", "-o", str(binary)])
    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = f"{sysroot}/lib:{sysroot}/lib/rustlib/x86_64-unknown-linux-gnu/lib"
    output = run([str(binary), "abi-input.rs", "--crate-type=lib", "--sysroot", sysroot], env=env)
    for expected in ["probe_scalar: convention=Rust", "mode=Pair", "mode: Pointer", "mode: OnStack"]:
        if expected not in output:
            raise RuntimeError(f"ABI observation changed: missing {expected}")
    result = {"toolchain": version, "observed_abi": output}
    (OUT / "abi-results.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
