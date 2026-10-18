#!/usr/bin/env python3
"""Fuzz std.http.server with libFuzzer, seeded from the request corpus.

Builds fuzz/http_server/main.plenty with `plenty --fuzz`, writes the cases of
tests/fixtures/http_server/corpus.txt as seeds, and runs the target. Inputs
that crash it are saved in WORK/findings; only those of this run are reported
and set the exit status. See
book/src/design/stdlib/http-qualification.md#fuzzing.

    python3 fuzz/http_server/run.py --plenty target/release/plenty --time 600 --jobs 2
"""

import argparse
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CORPUS = ROOT / "tests" / "fixtures" / "http_server" / "corpus.txt"

# The target's first byte selects its read size, `byte % 64 + 1`, and with
# 64 small limits: each case is seeded in 64-byte reads (the largest step), in
# 7-byte reads, one byte at a time, and in 7-byte reads with small limits.
STEPS = (63, 6, 0, 70)


def unescape(text):
    """The corpus escapes: \\r \\n \\t \\\\ \\xHH and \\{N*piece}."""
    out = bytearray()
    i = 0
    while i < len(text):
        c = text[i]
        if c != "\\":
            out += c.encode()
            i += 1
            continue
        kind = text[i + 1]
        if kind in "rnt\\":
            out += {"r": b"\r", "n": b"\n", "t": b"\t", "\\": b"\\"}[kind]
            i += 2
        elif kind == "x":
            out.append(int(text[i + 2 : i + 4], 16))
            i += 4
        elif kind == "{":
            end = text.index("}", i)
            count, piece = text[i + 2 : end].split("*", 1)
            out += unescape(piece) * int(count)
            i = end + 1
        else:
            raise ValueError(f"unknown escape \\{kind}")
    return bytes(out)


def seeds():
    """The request bytes of every corpus case."""
    for line in CORPUS.read_text().splitlines():
        if line and not line.startswith("#"):
            _, raw = line.split(" |", 1)
            yield unescape(raw.removeprefix(" "))


def write_seeds(directory):
    directory.mkdir(parents=True, exist_ok=True)
    count = 0
    for case, request in enumerate(seeds()):
        for step in STEPS:
            (directory / f"case-{case:03}-{step}").write_bytes(bytes([step]) + request)
            count += 1
    return count


def build(plenty, output, release):
    command = [str(plenty)]
    if release:
        command.append("--release")
    command += ["--fuzz", str(HERE / "main.plenty"), "-o", str(output)]
    subprocess.run(command, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--plenty", default=ROOT / "target" / "release" / "plenty", type=pathlib.Path)
    parser.add_argument("--work", default=ROOT / "target" / "fuzz-http-server", type=pathlib.Path)
    parser.add_argument("--time", default=60, type=int, help="seconds to fuzz (default 60)")
    parser.add_argument("--runs", type=int, help="stop after this many inputs instead")
    parser.add_argument("--jobs", default=1, type=int, help="fuzzing processes (default 1)")
    parser.add_argument("--seed", type=int, help="libFuzzer's random seed")
    parser.add_argument("--debug-build", action="store_true", help="build without --release")
    parser.add_argument("extra", nargs="*", help="more libFuzzer flags, after --")
    args = parser.parse_args()

    work = args.work.resolve()
    target = work / "http_server_fuzzer"
    corpus = work / "corpus"
    findings = work / "findings"
    corpus.mkdir(parents=True, exist_ok=True)
    findings.mkdir(parents=True, exist_ok=True)
    # Findings of earlier runs stay in place; only this run's count.
    earlier = set(findings.iterdir())
    build(args.plenty, target, not args.debug_build)
    count = write_seeds(work / "seeds")
    print(f"{count} seeds from {CORPUS.relative_to(ROOT)}", flush=True)

    command = [
        str(target),
        f"-dict={HERE / 'http.dict'}",
        f"-artifact_prefix={findings}/",
        "-max_len=4096",
        "-timeout=10",
        "-print_final_stats=1",
    ]
    if args.runs is not None:
        command.append(f"-runs={args.runs}")
    else:
        command.append(f"-max_total_time={args.time}")
    if args.seed is not None:
        command.append(f"-seed={args.seed}")
    if args.jobs > 1:
        # Independent processes sharing the corpus directory, each logging to
        # WORK/fuzz-N.log. Not -fork: its parent process crashes when the
        # memcmp interceptors are linked.
        command += [f"-jobs={args.jobs}", f"-workers={args.jobs}"]
    command += args.extra
    command += [str(corpus), str(work / "seeds")]
    status = subprocess.run(command, cwd=work).returncode
    found = sorted(set(findings.iterdir()) - earlier)
    if earlier:
        print(f"{len(earlier)} earlier findings in {findings}")
    for path in found:
        print(f"finding: {path}")
    return status if status != 0 or not found else 1


if __name__ == "__main__":
    sys.exit(main())
