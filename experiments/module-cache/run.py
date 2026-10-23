#!/usr/bin/env python3
"""Bounded native-module reuse experiment; not Plenty's module cache.

Uses existing scalar C exports as an explicit module boundary. Generated
interfaces are fixture-owned, not extracted from arbitrary Plenty syntax.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import tempfile
import time


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fixtures(root, count, functions, variant):
    interfaces = {}
    for i in range(count):
        extra = ", step: i64" if i == 0 and variant == "interface" else ""
        body = "x + step" if extra else f"x + {2 if i == 0 and variant == 'body' else 1}"
        declaration = f'def value(x: i64{extra}) -> i64 = "m{i}_value"'
        interfaces[f"m{i}"] = f"pub extern {declaration}\n"
        text = f"export {declaration}:\n    {body}\n"
        for f in range(functions):
            text += f"\ndef f_{f}(x: i64) -> i64:\n    x + {f}\n"
        (root / f"m{i}.plenty").write_text(text)
        (root / f"api{i}.plentyi").write_text(interfaces[f"m{i}"])
    arg = "40, 3" if variant == "interface" else "40"
    (root / "caller.plenty").write_text(
        'import api0\nexport def answer() -> i64 = "caller_answer":\n'
        f"    api0.value({arg}) + 1\n")
    interfaces["caller"] = 'pub extern def answer() -> i64 = "caller_answer"\n'
    (root / "answer.plentyi").write_text(interfaces["caller"])
    (root / "main.plenty").write_text(
        "import answer\ndef main() -> Result[(), IoError]:\n"
        "    print(answer.answer())?\n    Ok(())\n")
    return interfaces


class Builder:
    def __init__(self, compiler, root, cache, objects):
        self.compiler, self.root, self.cache = compiler, root, cache
        self.objects = objects
        self.identity = digest(compiler.read_bytes())
        self.env = dict(os.environ, PLENTY_CACHE="off")
        self.env.pop("PLENTY_PROFILE", None)
        self.rss = []

    def command(self, args):
        subprocess.run(["/usr/bin/time", "-f", "%M", "-o", str(self.root / "rss"),
                        str(self.compiler), *map(str, args)], env=self.env,
                       capture_output=True, text=True, check=True)
        self.rss.append(int((self.root / "rss").read_text()))

    def build(self, interfaces, fresh=False):
        start = time.perf_counter()
        self.rss = []
        hits, misses, archives = [], [], []
        for name in interfaces:
            source = self.root / f"{name}.plenty"
            dependencies = {"m0": interfaces["m0"]} if name == "caller" else {}
            key = digest(json.dumps({"format": 1, "compiler": self.identity,
                                     "source": source.read_text(), "name": name,
                                     "dependencies": dependencies,
                                     "mode": "static-library/default-target/default-opt",
                                     "objects": self.objects},
                                    sort_keys=True).encode())
            directory = self.cache / key
            archive = directory / f"{name}.{'o' if self.objects else 'a'}"
            checksum = directory / "sha256"
            valid = (not fresh and archive.exists() and checksum.exists()
                     and digest(archive.read_bytes()) == checksum.read_text())
            if valid:
                hits.append(name)
            else:
                misses.append(name)
                directory.mkdir(parents=True, exist_ok=True)
                packaged = directory / f"{name}.a"
                self.command(["--static-library", source, "--library-name", name,
                              "-o", packaged])
                if self.objects:
                    # Existing public library output is the experimental bridge;
                    # retain only its program object, sharing the final runtime.
                    result = subprocess.run(["ar", "p", str(packaged), "plenty_exports.o"],
                                            capture_output=True, check=True)
                    archive.write_bytes(result.stdout)
                    packaged.unlink()
                checksum.write_text(digest(archive.read_bytes()))
            archives.append(archive)
        # The consumer precedes its dependencies for an ordinary static linker.
        archives.reverse()
        args = ["--compile", self.root / "main.plenty", "-o", self.root / "app"]
        for archive in archives:
            args += ["--link-arg", archive]
        self.command(args)
        elapsed = time.perf_counter() - start
        output = subprocess.run([str(self.root / "app")], check=True,
                                capture_output=True, text=True).stdout
        return {"seconds": elapsed, "peak_child_rss_kib": max(self.rss),
                "hits": hits, "misses": misses, "output": output,
                "archive_bytes": sum(p.stat().st_size for p in archives),
                "executable_bytes": (self.root / "app").stat().st_size,
                "executable_sha256": digest((self.root / "app").read_bytes())}

    def monolithic(self, interfaces):
        directory = self.root / "ordinary"
        directory.mkdir(exist_ok=True)
        for name in interfaces:
            source = (self.root / f"{name}.plenty").read_text()
            source = re.sub(r'export (def .*?) = "[^"]+":', r'pub \1:', source)
            source = source.replace("import api0", "import m0").replace("api0.value", "m0.value")
            (directory / f"{name}.plenty").write_text(source)
        (directory / "main.plenty").write_text(
            (self.root / "main.plenty").read_text().replace("import answer", "import caller")
            .replace("answer.answer", "caller.answer"))
        # Keep all the independent modules in the ordinary compilation too.
        main = directory / "main.plenty"
        main.write_text("".join(f"import {name}\n" for name in interfaces if name != "caller")
                        + main.read_text())
        self.rss = []
        start = time.perf_counter()
        self.command(["--compile", main, "-o", directory / "app"])
        elapsed = time.perf_counter() - start
        output = subprocess.run([str(directory / "app")], check=True,
                                capture_output=True, text=True).stdout
        return {"seconds": elapsed, "peak_child_rss_kib": max(self.rss), "output": output,
                "archive_bytes": 0, "executable_bytes": (directory / "app").stat().st_size}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path)
    parser.add_argument("--modules", type=int, default=10)
    parser.add_argument("--functions", type=int, default=100)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--objects", action="store_true",
                        help="keep only each archive's program object")
    parser.add_argument("--json", type=Path, required=True)
    args = parser.parse_args()
    if min(args.modules, args.functions, args.runs) < 1:
        parser.error("counts must be positive")
    compiler = args.compiler.resolve(strict=True)
    rows = []
    with tempfile.TemporaryDirectory(prefix="plenty-module-artifacts-") as temp:
        root = Path(temp)
        for repetition in range(args.runs):
            builder = Builder(compiler, root, root / f"cache-{repetition}", args.objects)
            row = {}
            interfaces = fixtures(root, args.modules, args.functions, "base")
            row["cold"] = builder.build(interfaces)
            row["monolithic_cold"] = builder.monolithic(interfaces)
            row["unchanged"] = builder.build(interfaces)
            assert len(row["cold"]["misses"]) == args.modules + 1
            assert row["unchanged"]["misses"] == []
            assert row["cold"]["output"] == row["unchanged"]["output"] == "42\n"
            assert row["monolithic_cold"]["output"] == "42\n"
            for variant, expected in [("body", "43\n"), ("interface", "44\n")]:
                interfaces = fixtures(root, args.modules, args.functions, variant)
                row[variant] = builder.build(interfaces)
                row[f"monolithic_{variant}"] = builder.monolithic(interfaces)
                assert row[f"monolithic_{variant}"]["output"] == expected
                assert row[variant]["misses"] == (["m0"] if variant == "body" else ["m0", "caller"])
                fresh = builder.build(interfaces, fresh=True)
                assert row[variant]["output"] == fresh["output"] == expected
                assert row[variant]["executable_sha256"] == fresh["executable_sha256"]
                interfaces = fixtures(root, args.modules, args.functions, "base")
                builder.build(interfaces)
            if repetition == 0:
                # Integrity failure is a miss, not silent stale native code.
                for archive in builder.cache.glob("*/*.o" if args.objects else "*/*.a"):
                    archive.write_bytes(b"damaged")
                recovered = builder.build(interfaces)
                assert len(recovered["misses"]) == args.modules + 1
                assert recovered["output"] == "42\n"
            rows.append(row)
    report = {"compiler_sha256": digest(compiler.read_bytes()),
              "cpu_affinity": sorted(os.sched_getaffinity(0)),
              "modules": args.modules, "functions": args.functions,
              "objects": args.objects, "samples": rows}
    args.json.write_text(json.dumps(report, indent=2) + "\n")
    for mode in ["cold", "unchanged", "body", "interface", "monolithic_cold", "monolithic_body", "monolithic_interface"]:
        print(mode, {key: statistics.median(row[mode][key] for row in rows)
                     for key in ["seconds", "peak_child_rss_kib", "archive_bytes", "executable_bytes"]})
    print("reuse, edit invalidation, fresh-build identity, execution and corruption checks passed")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        raise SystemExit(f"command failed: {error.cmd}\n{error.stdout}\n{error.stderr}") from error
