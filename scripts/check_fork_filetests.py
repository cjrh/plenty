#!/usr/bin/env python3
"""Compare Cranelift x64 filetests with its registry and Plenty's pinned allocator.

Uses local Git objects only, exports isolated snapshots, never blesses tests,
and keeps logs/commands/dependency trees under target/fork-filetests/.
"""

import argparse
from datetime import datetime, timezone
import json
import os
import shutil
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib

REPO = Path(__file__).resolve().parent.parent


def pinned_revision(lockfile, package, repository):
    matches = {
        p.get("source", "")
        for p in tomllib.loads(lockfile.read_text())["package"]
        if p["name"] == package
    }
    prefix = f"git+https://github.com/plentylang/{repository}?branch=fork#"
    if len(matches) != 1 or not next(iter(matches)).startswith(prefix):
        raise ValueError(f"{package}: expected one locked {repository} fork revision")
    revision = matches.pop().removeprefix(prefix)
    if len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
        raise ValueError(f"{package}: invalid locked revision")
    return revision


def snapshot(repository, revision, destination):
    """Export committed state; dirty files, Git metadata and submodules stay out."""
    destination.mkdir()
    with tempfile.TemporaryFile() as archive:
        subprocess.run(["git", "-C", str(repository), "archive", "--format=tar", revision],
                       stdout=archive, check=True)
        archive.seek(0)
        with tarfile.open(fileobj=archive) as tree:
            tree.extractall(destination, filter="data")


def check_environment():
    env = os.environ.copy()
    # An inherited blessing setting must never rewrite even our exported tests.
    env.pop("CRANELIFT_TEST_BLESS", None)
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    env.pop("RUSTFLAGS", None)
    env["RUSTC_WRAPPER"] = ""
    env["RUSTC_WORKSPACE_WRAPPER"] = ""
    return env


def run_logged(command, cwd, env, log):
    with log.open("w") as output:
        result = subprocess.run(command, cwd=cwd, env=env, stdout=output,
                                stderr=subprocess.STDOUT, check=False)
    return result.returncode


def verify_allocator(log, allocator):
    roots = [line for line in log.read_text().splitlines() if line.startswith("regalloc2 v")]
    if len(roots) != 1:
        raise ValueError("dependency tree does not identify exactly one regalloc2")
    if allocator is None:
        valid = " (" not in roots[0]
    else:
        valid = roots[0].endswith(f" ({allocator})")
    if not valid:
        raise ValueError(f"unexpected allocator dependency: {roots[0]}")


def check(args):
    pins = {
        "wasmtime": pinned_revision(REPO / "Cargo.lock", "cranelift-codegen", "wasmtime"),
        "regalloc2": pinned_revision(REPO / "Cargo.lock", "regalloc2", "regalloc2"),
    }
    args.output.mkdir(parents=True, exist_ok=True)
    report = Path(tempfile.mkdtemp(prefix="run-", dir=args.output)).resolve()
    print(f"Report: {report}", flush=True)
    summary = {"pins": pins, "started_at": datetime.now(timezone.utc).isoformat(),
               "runs": [], "status": "setup-failed"}
    try:
        source = report / "wasmtime"
        allocator = report / "regalloc2"
        snapshot(args.wasmtime, pins["wasmtime"], source)
        snapshot(args.regalloc2, pins["regalloc2"], allocator)
        env = check_environment()
        env["CARGO_TARGET_DIR"] = str(report / "build")
        # Separate target directory from the main compiler and ordinary fork builds.
        env["CARGO_PROFILE_DEV_DEBUG"] = "0"
        for name, patch in [("registry", []), ("plenty", ["--config",
                f"patch.crates-io.regalloc2.path={json.dumps(str(allocator))}"])]:
            cargo = ["cargo", *patch]
            tree = cargo + ["tree", "--offline", "-p", "cranelift-tools", "--no-default-features",
                            "--features", "cranelift-codegen/x86", "--prefix", "none", "-i", "regalloc2"]
            build = cargo + ["build", "--offline", "-p", "cranelift-tools", "--bin",
                             "clif-util", "--no-default-features", "--features",
                             "cranelift-codegen/x86"]
            test = [str(Path(env["CARGO_TARGET_DIR"]) / "debug/clif-util"), "test",
                    "cranelift/filetests/filetests/isa/x64"]
            run = {"allocator": name, "commands": [tree, build, test]}
            summary["runs"].append(run)
            for phase, command in [("tree", tree), ("build", build), ("test", test)]:
                print(f"{name}: {phase}", flush=True)
                run[phase + "_exit"] = run_logged(command, source, env,
                                                  report / f"{name}-{phase}.log")
                if run[phase + "_exit"] and phase != "test":
                    summary["status"] = f"{name}-{phase}-failed"
                    return 2
                if phase == "tree":
                    shutil.copyfile(source / "Cargo.lock", report / f"{name}-Cargo.lock")
                    verify_allocator(report / f"{name}-tree.log",
                                     allocator if name == "plenty" else None)
        baseline, patched = summary["runs"]
        if baseline["test_exit"]:
            summary["status"] = "registry-filetests-failed"
            return 2
        if patched["test_exit"]:
            summary["status"] = "allocator-differences-or-failures"
            return 1
        summary["status"] = "passed"
        return 0
    finally:
        (report / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(f"Result: {summary['status']}; inspect logs and summary.json in {report}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    # Resolve primary checkout via Git, because a task worktree is often in /tmp.
    common = Path(subprocess.check_output(
        ["git", "-C", str(REPO), "rev-parse", "--path-format=absolute", "--git-common-dir"],
        text=True).strip()).parent
    parser.add_argument("--wasmtime", type=Path, default=common.parent / "wasmtime")
    parser.add_argument("--regalloc2", type=Path, default=common.parent / "regalloc2")
    parser.add_argument("--output", type=Path, default=REPO / "target/fork-filetests")
    args = parser.parse_args()
    try:
        return check(args)
    except (OSError, ValueError, subprocess.CalledProcessError, tarfile.TarError) as error:
        print(f"check-fork-filetests: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
