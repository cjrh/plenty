#!/usr/bin/env python3
"""Link a Plenty application object with one Cargo support staticlib (Linux GNU)."""
import argparse
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys


def native_link_inputs(stdout: str, manifest: Path, target_name: str) -> tuple[Path, list[str]]:
    """Use this invocation's matching Cargo records, including cached artifacts."""
    path = str(manifest.resolve())
    messages = []
    for line in stdout.splitlines():
        message = json.loads(line)
        target = message.get("target", {})
        if (message.get("manifest_path") == path
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
    parser.add_argument("--manifest-path", type=Path, required=True)
    parser.add_argument("--lib-name", required=True, help="Cargo library target name")
    parser.add_argument("--target", help="pass an explicit target triple to Cargo")
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--features", help="comma-separated support crate features")
    parser.add_argument("--require-contract", metavar="GUARD",
                        help="require the reviewed NAME_plenty_contract_v1_HASH symbol at final link")
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--print-inputs", action="store_true", help="emit archive and ordered link arguments as JSON instead of linking")
    parser.add_argument("-o", type=Path)
    parser.add_argument("objects", nargs="*", type=Path, help="Plenty object, then any native objects")
    args = parser.parse_args()
    if args.print_inputs and (args.objects or args.o or args.require_contract):
        parser.error("--print-inputs excludes link options")
    if not args.print_inputs and not (args.objects and args.o):
        parser.error("linking requires objects and -o OUTPUT")
    guard = []
    if args.require_contract is not None:
        if not re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*_plenty_contract_v1_[a-f0-9]{64}", args.require_contract):
            parser.error("--require-contract expects the full reviewed contract guard symbol")
        guard = [f"-Wl,--require-defined={args.require_contract}"]
    command = ["cargo", "rustc", "--locked", "--manifest-path", args.manifest_path,
               "--lib", "--message-format=json"]
    if args.target:
        command += ["--target", args.target]
    if args.target_dir:
        command += ["--target-dir", args.target_dir]
    if args.features:
        command += ["--features", args.features]
    if args.release:
        command.append("--release")
    command += ["--", "--print", "native-static-libs"]
    result = subprocess.run(command, stdout=subprocess.PIPE, text=True)
    if result.returncode:
        # Cargo stderr was inherited, but compiler diagnostics are in its JSON stream.
        sys.stderr.write(result.stdout)
        raise SystemExit(result.returncode)
    try:
        archive, native_args = native_link_inputs(result.stdout, args.manifest_path, args.lib_name)
    except (ValueError, KeyError) as error:
        raise SystemExit(str(error)) from error
    if args.print_inputs:
        print(json.dumps([str(archive), native_args]))
        return
    linked = subprocess.run(["cc", *args.objects, archive, "-Wl,--gc-sections", *guard, *native_args,
                             "-o", args.o])
    if linked.returncode:
        raise SystemExit(linked.returncode)


if __name__ == "__main__":
    main()
