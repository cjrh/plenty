#!/usr/bin/env python3
"""Measure fresh-process warm script launches, with a fixed cache environment.

Run from a quiet machine with release compiler binaries. Diagnostics and cache
population are outside the timed samples; raw samples include child CPU usage.
This Linux investigation helper needs no third-party Python packages.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import resource
import statistics
import subprocess
import tempfile
import time


SOURCE = "#!/usr/bin/env -S plenty script\ndef main() -> Result[(), IoError]:\n    Ok(())\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compilers", nargs="+", type=Path)
    parser.add_argument("--runs", type=int, default=100)
    parser.add_argument("--warmup", type=int, default=10)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.runs < 2 or args.warmup < 1:
        parser.error("--runs must be at least 2 and --warmup at least 1")
    compilers = [p.resolve(strict=True) for p in args.compilers]
    report = {"platform": platform.platform(), "source": SOURCE,
              "runs": args.runs, "warmup": args.warmup, "results": []}
    with tempfile.TemporaryDirectory(prefix="plenty-startup-") as tmp:
        root = Path(tmp)
        work = root / "work"
        work.mkdir()
        bin_dir = root / "bin"
        bin_dir.mkdir()
        script = work / "hello.plenty"
        script.write_text(SOURCE)
        script.chmod(0o700)
        launcher = bin_dir / "plenty"
        aot = work / "aot"
        env = {
            "HOME": os.environ["HOME"],
            "PATH": f"{bin_dir}:/usr/bin:/bin",
            "PLENTY_CACHE_DIR": str(root / "cache"),
            "TMPDIR": str(root),
        }

        def run(command):
            return subprocess.run(command, cwd=work, env=env, capture_output=True, check=True)

        def hit():
            diagnostics = run([launcher, "script", "--cache-stats", script]).stderr.decode()
            if "program hit" not in diagnostics or "executable cache: hit" not in diagnostics:
                raise RuntimeError(f"benchmark requires confirmed executable reuse: {diagnostics}")
            return diagnostics

        for compiler in compilers:
            with compiler.open("rb") as binary:
                digest = hashlib.file_digest(binary, "sha256").hexdigest()
            launcher.unlink(missing_ok=True)
            launcher.symlink_to(compiler)
            version = run([launcher, "--version"]).stdout.decode().strip()
            run([launcher, "script", "--compile", "-o", aot, script])
            run([launcher, "script", script])
            diagnostics = hit()
            commands = {
                "true": ["/usr/bin/true"],
                "version": [launcher, "--version"],
                "aot": [aot],
                "direct": [launcher, "script", script],
                "shebang": [script],
            }
            samples = {name: [] for name in commands}
            for _ in range(args.warmup):
                for command in commands.values():
                    run(command)
            rng = random.Random(699)
            for _ in range(args.runs):
                order = list(commands)
                rng.shuffle(order)
                for name in order:
                    before = resource.getrusage(resource.RUSAGE_CHILDREN)
                    started = time.perf_counter_ns()
                    run(commands[name])
                    elapsed = time.perf_counter_ns() - started
                    after = resource.getrusage(resource.RUSAGE_CHILDREN)
                    samples[name].append({
                        "wall_ms": elapsed / 1e6,
                        "user_ms": (after.ru_utime - before.ru_utime) * 1000,
                        "system_ms": (after.ru_stime - before.ru_stime) * 1000,
                    })
            hit()
            summary = {}
            for name, values in samples.items():
                walls = sorted(s["wall_ms"] for s in values)
                summary[name] = {
                    "median_ms": statistics.median(walls),
                    "p10_ms": walls[int((len(walls) - 1) * 0.1)],
                    "p90_ms": walls[int((len(walls) - 1) * 0.9)],
                    "user_ms": statistics.median(s["user_ms"] for s in values),
                    "system_ms": statistics.median(s["system_ms"] for s in values),
                }
            report["results"].append({
                "compiler": str(compiler), "version": version, "sha256": digest,
                "diagnostics": diagnostics, "summary": summary, "samples": samples,
            })
            print(version, compiler)
            for name, values in summary.items():
                print(f"  {name:8} {values['median_ms']:.3f} ms "
                      f"(p10–p90 {values['p10_ms']:.3f}–{values['p90_ms']:.3f}), "
                      f"user {values['user_ms']:.3f}, sys {values['system_ms']:.3f}")
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
