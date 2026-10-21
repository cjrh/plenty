"""Build equivalent C/Plenty kernels and retain assembly for manual review."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def run(args, **kwargs):
    result = subprocess.run(args, capture_output=True, text=True, **kwargs)
    if result.returncode:
        raise RuntimeError(f"{args!r} failed ({result.returncode}):\n{result.stdout}{result.stderr}")
    return result.stdout


def functions(binary):
    result = {}
    for line in run(["nm", "-S", "--defined-only", str(binary)]).splitlines():
        fields = line.split()
        if len(fields) == 4 and fields[2].lower() == "t":
            result[fields[3]] = int(fields[1], 16)
    return result


def main():
    parser = argparse.ArgumentParser(__doc__)
    parser.add_argument("--compiler", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--capture-ir", action="store_true", help="Requires temporary PLENTY_AUDIT_IR instrumentation")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    compiler = args.compiler.resolve()
    sizes = {}
    outputs = {}
    commands = []
    for name in ("kernels", "collections"):
        env = dict(os.environ)
        if args.capture_ir:
            ir = output / (name + "-ir")
            ir.mkdir(exist_ok=True)
            env["PLENTY_AUDIT_IR"] = str(ir)
        binary = output / (name + "-plenty")
        command = [str(compiler), "--release", "--no-cache", "--compile", str(root / (name + ".plenty")), "-o", str(binary)]
        commands.append(command)
        run(command, env=env)
        outputs[binary.name] = run([str(binary)], timeout=60)
        sizes[binary.name] = functions(binary)
        (output / (binary.name + ".asm")).write_text(run(["objdump", "-d", "--no-show-raw-insn", str(binary)]))
    expected = 1024 * (400000 // 2048) + min(1024, 400000 % 2048)
    assert outputs["collections-plenty"].strip() == str(expected)
    for level in ("O2", "O3"):
        binary = output / ("kernels-c-" + level)
        command = ["gcc", "-" + level, "-march=x86-64", "-mtune=generic", "-ffp-contract=off", "-fno-omit-frame-pointer", "-Wall", "-Wextra", "-Werror", str(root / "kernels.c"), "-o", str(binary)]
        commands.append(command)
        run(command)
        outputs[binary.name] = run([str(binary)])
        assert outputs[binary.name] == outputs["kernels-plenty"], outputs
        sizes[binary.name] = functions(binary)
        (output / (binary.name + ".asm")).write_text(run(["objdump", "-d", "--no-show-raw-insn", str(binary)]))
    names = ("div10", "rem10", "mix", "escape", "sum_words", "sum_indexed")
    summary = {variant: {name: symbols.get(("__plenty_fn_" if variant.endswith("plenty") else "") + name) for name in names} for variant, symbols in sizes.items() if variant.startswith("kernels")}
    metadata = {"commands": commands, "gcc": run(["gcc", "--version"]).splitlines()[0], "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(), "capture_ir": args.capture_ir, "experimental_fold": "PLENTY_AUDIT_FOLD" in os.environ, "outputs": outputs, "function_bytes": summary}
    (output / "results.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps({"function_bytes": summary, "outputs_match": True, "collection_checksum": expected}, indent=2))


if __name__ == "__main__":
    main()
