"""Run isolated SIMD evidence, preserving generated objects and compiler diagnostics."""
import argparse
import json
import os
from pathlib import Path
import platform
import subprocess
import time

ROOT = Path(__file__).resolve().parent


def run(args, log, *, allow_failure=False):
    started = time.perf_counter()
    result = subprocess.run(args, cwd=ROOT, text=True, capture_output=True, check=False,
                            env={**os.environ, "PLENTY_CACHE": "off"})
    elapsed = time.perf_counter() - started
    log.write_text(result.stdout + result.stderr)
    if result.returncode and not allow_failure:
        raise RuntimeError(f"{args!r} failed: see {log}")
    return {"command": args, "seconds": elapsed, "exit": result.returncode}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=Path("/tmp/plenty-simd-results"))
    parser.add_argument("--plenty", type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    records = []
    records.append(run(["cargo", "build", "--release", "--locked", "--offline"], out / "build.log"))
    exe = ROOT / "target/release/plenty-simd-spike"
    for target, label in [("x86_64-unknown-linux-gnu", "x64"), ("aarch64-unknown-linux-gnu", "arm64")]:
        for lanes in [1, 4, 8]:
            records.append(run([str(exe), target, str(lanes), str(out / f"{label}-{lanes}.o")],
                               out / f"{label}-{lanes}.log", allow_failure=lanes == 8))
        records.append(run(["clang", "--no-default-config", "-O3", "-ffp-contract=off", "-Rpass=loop-vectorize",
                            "-Rpass-missed=loop-vectorize", "-target", target, "-S",
                            "kernels.c", "-o", str(out / f"llvm-{label}.s")], out / f"llvm-{label}.log"))
    records.append(run(["clang", "--no-default-config", "-target", "x86_64-unknown-linux-gnu", "-O3", "-ffp-contract=off", "-c", "kernels.c", "-o", str(out / "llvm.o")], out / "llvm-build.log"))
    records.append(run(["clang", "--no-default-config", "-O2", "-ffp-contract=off", "driver.c", str(out / "x64-1.o"),
                        str(out / "x64-4.o"), str(out / "llvm.o"), "-o", str(out / "check")], out / "link.log"))
    records.append(run([str(out / "check")], out / "runtime.log"))
    for lanes in [1, 4]:
        records.append(run(["objdump", "-d", str(out / f"x64-{lanes}.o")], out / f"x64-{lanes}.asm"))
    if args.plenty:
        records.append(run([str(args.plenty.resolve()), "pipelines.plenty"], out / "plenty.log"))
        if (out / "plenty.log").read_text().strip() != "1505500 500 1.0":
            raise RuntimeError(f"unexpected Plenty result: see {out / 'plenty.log'}")
    metadata = {"platform": platform.platform(), "machine": platform.machine(), "commands": records}
    (out / "commands.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print((out / "runtime.log").read_text(), end="")
    print(f"Objects, assembly, remarks, status and timings: {out}")


if __name__ == "__main__":
    main()
