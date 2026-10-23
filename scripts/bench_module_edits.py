#!/usr/bin/env python3
"""Measure fresh-process builds and validate edit invalidation.

Pass one or more release compiler binaries; their run order alternates. Each
sample uses an isolated cache and the same source path. GNU time supplies peak
RSS on Linux. JSON retains every timing/cache result, not only medians. This is
an additive workload: it does not change compile_bench's historical template.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


def sources(root, modules, functions, variant):
    """One edited dependency, a caller and otherwise unchanged modules."""
    extra = ", step: i64" if variant == "interface" else ""
    body = "x + step" if variant == "interface" else f"x + {2 if variant == 'body' else 1}"
    generic = "x + x" if variant == "generic" else "x"
    for module in range(modules):
        text = ""
        if module == 0:
            text = (f"pub def value(x: i64{extra}) -> i64:\n    {body}\n\n"
                    f"pub def identity[T: IntType](x: T) -> T:\n    {generic}\n\n")
        for function in range(functions):
            text += f"pub def f_{function}(x: i64) -> i64:\n    x + {function}\n\n"
        (root / f"part{module}.plenty").write_text(text)
    arg = "40, 3" if variant == "interface" else "40"
    (root / "caller.plenty").write_text(
        "import part0\n\npub def answer() -> i64:\n"
        f"    part0.value({arg}) + part0.identity[i64](1)\n")


def run(compiler, root, cache, cache_off=False, output="program.o", profile=False, release=False):
    env = dict(os.environ)
    for name in ["PLENTY_CACHE", "PLENTY_CACHE_DIR", "PLENTY_PROFILE",
                 "HOTPATH_OUTPUT_PATH", "HOTPATH_OUTPUT_FORMAT"]:
        env.pop(name, None)
    env["PLENTY_CACHE_DIR"] = str(cache)
    if cache_off:
        env["PLENTY_CACHE"] = "off"
    if profile:
        env.update(PLENTY_PROFILE="1", HOTPATH_OUTPUT_PATH=str(root / "profile.json"),
                   HOTPATH_OUTPUT_FORMAT="json")
    command = [str(compiler)] + (["--release"] if release else [])
    command += ["--cache-stats", "--compile" if release else "--emit-object", str(root / "main.plenty"),
               "-o", str(root / output)]
    start = time.perf_counter()
    result = subprocess.run(["/usr/bin/time", "-f", "%M", "-o", str(root / "rss"), *command],
                            env=env, capture_output=True, text=True, check=True)
    row = {
        "seconds": time.perf_counter() - start,
        "peak_rss_kib": int((root / "rss").read_text()),
        "artifact_bytes": (root / output).stat().st_size,
        "cache": result.stderr.strip(),
        "sha256": hashlib.sha256((root / output).read_bytes()).hexdigest(),
    }
    if profile and (root / "profile.json").exists():
        row["profile"] = json.loads((root / "profile.json").read_text())
        (root / "profile.json").unlink()
    return row


def verify_execution(compiler, root, variant, release):
    env = dict(os.environ, PLENTY_CACHE="off")
    env.pop("PLENTY_PROFILE", None)
    executable = root / "program"
    subprocess.run([str(compiler)] + (["--release"] if release else [])
                   + ["--compile", str(root / "main.plenty"), "-o", str(executable)],
                   env=env, check=True, capture_output=True, text=True)
    output = subprocess.run([str(executable)], check=True, capture_output=True, text=True).stdout
    expected = {"base": "42\n", "body": "43\n", "interface": "44\n", "generic": "43\n"}[variant]
    if output != expected:
        raise RuntimeError(f"{variant}: expected {expected!r}, got {output!r}")


def sample(compiler, root, cache, args, validate):
    def build(**kwargs):
        return run(compiler, root, cache, release=args.release, **kwargs)

    result = {}
    sources(root, args.modules, args.functions, "base")
    result["disabled"] = build(cache_off=True)
    result["populate"] = build()
    # The first request only writes a marker. The second stores the object.
    result["function_warm"] = build()
    result["unchanged"] = build()
    if "program hit" not in result["unchanged"]["cache"]:
        raise RuntimeError("unchanged build did not reuse its program object")
    for mode in ["populate", "function_warm", "unchanged"]:
        if result[mode]["sha256"] != result["disabled"]["sha256"]:
            raise RuntimeError(f"{mode} changed the object")
    if validate:
        verify_execution(compiler, root, "base", args.release)
    for variant in ["body", "interface", "generic"]:
        sources(root, args.modules, args.functions, variant)
        result[variant] = build()
        if "program miss" not in result[variant]["cache"]:
            raise RuntimeError(f"{variant} edit reused a stale program object")
        uncached = build(cache_off=True, output="uncached.o")
        if result[variant]["sha256"] != uncached["sha256"]:
            raise RuntimeError(f"{variant} edit changed the cache-on/off object")
        if validate:
            verify_execution(compiler, root, variant, args.release)
        # Each edit starts from the same base, with an actual base object.
        sources(root, args.modules, args.functions, "base")
        build()
    if args.profile and validate:
        # Profiles are separate invocations, outside the timed samples above.
        result["profile_disabled"] = build(cache_off=True, profile=True)
        sources(root, args.modules, args.functions, "body")
        result["profile_body"] = build(profile=True)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compilers", nargs="+", type=Path)
    parser.add_argument("--modules", type=int, default=10)
    parser.add_argument("--functions", type=int, default=100)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--import-std", default="std.http.server",
                        help="standard module imported by main; empty string disables it")
    parser.add_argument("--profile", action="store_true", help="also collect separate hotpath reports")
    parser.add_argument("--release", action="store_true", help="measure optimized executable builds, including linking")
    parser.add_argument("--json", required=True, type=Path)
    args = parser.parse_args()
    if min(args.modules, args.functions, args.runs) < 1:
        parser.error("modules, functions and runs must be positive")
    compilers = [p.resolve(strict=True) for p in args.compilers]
    if len(set(compilers)) != len(compilers):
        parser.error("compiler paths must be distinct")
    samples = {str(p): [] for p in compilers}
    with tempfile.TemporaryDirectory(prefix="plenty-module-edits-") as directory:
        root = Path(directory)
        imports = "".join(f"import part{i}\n" for i in range(args.modules))
        if args.import_std:
            imports += f"import {args.import_std}\n"
        (root / "main.plenty").write_text(imports + "import caller\n\n"
            "def main() -> Result[(), IoError]:\n    print(caller.answer())?\n    Ok(())\n")
        for repetition in range(args.runs):
            order = compilers if repetition % 2 == 0 else list(reversed(compilers))
            for compiler in order:
                cache = root / f"cache-{repetition}-{compilers.index(compiler)}"
                samples[str(compiler)].append(sample(compiler, root, cache, args, repetition == 0))
    report = {
        "workload": {"modules": args.modules, "functions_per_module": args.functions,
                     "import_std": args.import_std, "runs": args.runs, "release": args.release},
        "compiler_sha256": {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in compilers},
        "cpu_affinity": sorted(os.sched_getaffinity(0)),
        "samples": samples,
    }
    args.json.write_text(json.dumps(report, indent=2) + "\n")
    for compiler, rows in samples.items():
        print(compiler)
        for mode in ["disabled", "populate", "function_warm", "unchanged", "body", "interface", "generic"]:
            median = lambda key: statistics.median(row[mode][key] for row in rows)
            print(f"  {mode}: {median('seconds'):.6f}s, {median('peak_rss_kib'):.0f}KiB RSS, "
                  f"{median('artifact_bytes'):.0f} artifact bytes")
    print(f"all artifacts and edit invalidations verified; results: {args.json}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(f"command failed ({error.returncode}): {error.cmd}\n{error.stderr}") from error
