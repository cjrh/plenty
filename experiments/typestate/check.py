#!/usr/bin/env python3
"""Run the bounded typestate model and check external misuse diagnostics."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


EXPECTED = """close 1 3
close 2 2
close 3 2
close 4 2
close 5 3
partial 1 3
close 6 0
before dropping retry error
close 8 0
close 9 0
before scope exit
close 7 0
"""

WRITER = "writer = response.unanswered(1).begin(0).unwrap()\n"
REJECTIONS = {
    "write_before_begin": (
        "drop(response.unanswered(1).write(1))",
        "unknown method `write`",
    ),
    "finish_before_begin": (
        "drop(response.unanswered(1).finish())",
        "unknown method `finish`",
    ),
    "begin_twice": (
        "first = response.unanswered(1)\n"
        "drop(first.begin(0))\n"
        "drop(first.begin(0))",
        "use of moved binding `first`",
    ),
    "finish_twice": (
        WRITER + "writer.finish().unwrap()\n"
        "writer.finish().unwrap()",
        "use of moved binding `writer`",
    ),
    "write_after_finish": (
        WRITER + "writer.finish().unwrap()\n"
        "drop(writer.write(0))",
        "use of moved binding `writer`",
    ),
    "reuse_after_fallible_write": (
        WRITER + "drop(writer.write(1))\n"
        "drop(writer.finish())",
        "use of moved binding `writer`",
    ),
    "forge_writer": (
        "drop(response.Writer(Nothing, 0))",
        "private",
    ),
    "forge_unanswered": (
        "drop(response.Unanswered(Nothing))",
        "private",
    ),
    "read_private_transport": (WRITER + "drop(writer.wire)", "private"),
    "copy_writer": (WRITER + "drop(copy(writer))", "cannot be copied"),
    "consume_borrowed_writer": (
        WRITER + "borrowed = &writer\ndrop(borrowed.finish())",
        "expected response.Writer",
    ),
}

# B72 field extraction remains outside the consuming-method implementation.
BOUNDARIES = {
    "owned_field_extraction": (
        "class Resource:\n"
        "    value: i64\n"
        "class State:\n"
        "    resource: Resource\n"
        "def consume(state: State) -> Resource:\n"
        "    state.resource\n"
        "def main() -> ():\n"
        "    drop(consume(State(Resource(1))))\n",
        "cannot move out of a field",
    ),
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True,
                        help="Scratch directory on disk, e.g. repo/target/typestate")
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).resolve().parent
    env = dict(os.environ, PLENTY_CACHE="off", PLENTY_LINKER_THREADS="1")
    with tempfile.TemporaryDirectory(dir=args.output) as temporary:
        work = Path(temporary)
        shutil.copy2(source / "response.plenty", work)
        shutil.copy2(source / "scenarios.plenty", work)
        executable = work / "scenarios"
        built = subprocess.run(
            [str(compiler), "--compile", str(work / "scenarios.plenty"),
             "-o", str(executable)], env=env, capture_output=True, text=True,
            timeout=120,
        )
        if built.returncode:
            raise AssertionError(built.stderr)
        run = subprocess.run([str(executable)], capture_output=True, text=True,
                             timeout=10)
        if run.returncode or run.stdout != EXPECTED or run.stderr:
            raise AssertionError((run.returncode, run.stdout, run.stderr))
        print("PASS: nine owners cleaned exactly once; retries, partial write, "
              "cancellation, abandonment, and early propagation")
        probes = dict(BOUNDARIES)
        for name, (body, diagnostic) in REJECTIONS.items():
            program = "import response\ndef main() -> ():\n" + "\n".join(
                "    " + line for line in body.splitlines()
            ) + "\n"
            probes[name] = program, diagnostic
        for name, (program, diagnostic) in probes.items():
            path = work / f"{name}.plenty"
            path.write_text(program)
            checked = subprocess.run([str(compiler), "--check", str(path)],
                                     env=env, capture_output=True, text=True,
                                     timeout=30)
            if checked.returncode == 0 or diagnostic not in checked.stderr:
                raise AssertionError(f"{name}: expected {diagnostic!r}; "
                                     f"got {checked.returncode}: {checked.stderr}")
            print(f"PASS: {name}")
    print(f"Validated native execution and {len(probes)} compiler rejections")


if __name__ == "__main__":
    main()
