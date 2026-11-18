#!/usr/bin/env python3
"""Build one Plenty source file into the single Plenty executable with opt-in GPUI support (Linux x86-64)."""
import argparse
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess

PACKAGE = Path(__file__).resolve().parent
REPO = PACKAGE.parents[2]


def native_link_inputs(stdout: str, manifest: Path, target_name: str) -> tuple[Path, list[str]]:
    """Use this invocation's matching Cargo records, including cached artifacts."""
    messages = []
    for line in stdout.splitlines():
        message = json.loads(line)
        target = message.get("target", {})
        if (message.get("manifest_path") == str(manifest.resolve())
                and target.get("name") == target_name
                and "staticlib" in target.get("crate_types", [])):
            messages.append(message)
    artifacts = [message for message in messages if message["reason"] == "compiler-artifact"]
    reports = [message["message"]["message"].removeprefix("native-static-libs: ")
               for message in messages if message["reason"] == "compiler-message"
               and message["message"]["message"].startswith("native-static-libs: ")]
    if len(artifacts) != 1 or len(reports) != 1:
        raise ValueError(f"Cargo must report one staticlib and native-static-libs list for {target_name}")
    archives = [Path(filename) for filename in artifacts[0]["filenames"]
                if Path(filename).suffix == ".a"]
    if len(archives) != 1 or not archives[0].is_file():
        raise ValueError(f"Cargo must report one existing staticlib archive for {target_name}")
    # The structured message is uncolored; rendered diagnostics and stderr may
    # contain ANSI even when the artifact and its diagnostics are cached.
    return archives[0], shlex.split(reports[0])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="?", type=Path, default=PACKAGE / "examples/counter.plenty")
    parser.add_argument("--plenty", type=Path, default=REPO / "target/release/plenty")
    parser.add_argument("--fast-compile", action="store_true")
    parser.add_argument("--release", action="store_true", help="optimize the Rust native support archive too")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("this experimental launcher is qualified only for Linux x86-64")
    compiler = args.plenty.resolve(strict=True)
    source = args.source.resolve(strict=True)
    profile = "fast-compile" if args.fast_compile else "optimized"
    out = PACKAGE / "target" / profile
    out.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, out / "application.plenty")
    shutil.copyfile(PACKAGE / "src/lib.plenty", out / "gpui.plenty")
    shutil.copyfile(PACKAGE / "src/native.plentyi", out / "native.plentyi")
    command = [str(compiler), "--emit-object", str(out / "application.plenty"),
               "--module-root", str(out), "--executable-exports", "gpui_app",
               "-o", str(out / "application.o")]
    if args.fast_compile:
        command.append("--fast-compile")
    subprocess.run(command, check=True)
    env = {**os.environ, "PLENTY_GPUI_APP_DIR": str(out)}
    manifest = PACKAGE / "native/Cargo.toml"
    command = ["cargo", "rustc", "--locked", "--manifest-path", str(manifest),
               "--target-dir", str(PACKAGE / "native/target"), "--lib", "--features", "application",
               "--message-format=json"]
    if args.release:
        command.append("--release")
    command += ["--", "--print", "native-static-libs"]
    result = subprocess.run(command, env=env, text=True, capture_output=True)
    (out / "native-build.log").write_text(result.stdout + result.stderr)
    if result.returncode:
        raise SystemExit(f"native support build failed; see {out / 'native-build.log'}")
    try:
        archive, native_args = native_link_inputs(result.stdout, manifest, "plenty_gpui")
    except (ValueError, KeyError) as error:
        raise SystemExit(f"{error}; see {out / 'native-build.log'}") from error
    (out / "native-link-args.txt").write_text("\n".join(native_args) + "\n")
    executable = out / "plenty-gpui"
    # Exactly one Rust-built archive: never link the compiler's bundled runtime.
    command = ["cc", str(out / "application.o"), str(archive), "-Wl,--gc-sections",
               *native_args, "-o", str(executable)]
    (out / "link-command.txt").write_text(shlex.join(command) + "\n")
    subprocess.run(command, check=True)
    print(executable, flush=True)


if __name__ == "__main__":
    main()
