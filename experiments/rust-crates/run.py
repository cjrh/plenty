#!/usr/bin/env python3
"""Reproducible, deliberately small three-way Rust dependency experiment."""
import ast
import io
import json
from pathlib import Path
import statistics
import subprocess
import time
import tokenize

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "target" / "probe-results"
TARGET = ROOT / "target" / "debug"
PIN = "rustc 1.99.0 (b940084d7 2026-09-28)"
# This is the handwritten API selection contract, not Rust reflection.
BINDINGS = {
    "scalar": (["i64"], "i64"),
    "char_count": (["str"], "i64"),
    "identity": (["i64"], "i64"),
    "new_matcher": (["str"], "Matcher"),
    "match_count": (["&Matcher", "str"], "i64"),
    "drop_matcher": (["Matcher"], "i64"),
    "new_future": (["i64"], "Pending"),
    "poll_future": (["&mut Pending"], "i64"),
    "finish_future": (["Pending"], "i64"),
    "live_owners": ([], "i64"),
}


def command(args, *, stdin=None, expected=0):
    start = time.perf_counter()
    run = subprocess.run(args, cwd=ROOT, input=stdin, text=True, capture_output=True)
    elapsed = time.perf_counter() - start
    if run.returncode != expected:
        raise RuntimeError(f"{args}: exit {run.returncode}\n{run.stdout}\n{run.stderr}")
    return elapsed, run.stdout, run.stderr


def without_borrows(text):
    tokens = list(tokenize.generate_tokens(io.StringIO(text).readline))
    result = []
    skip_mut = False
    for token in tokens:
        if token.type == tokenize.OP and token.string == "&":
            skip_mut = True
            continue
        if skip_mut and token.string == "mut":
            skip_mut = False
            continue
        skip_mut = False
        result.append((token.type, token.string))
    return tokenize.untokenize(result)


def parse(source):
    lines = source.splitlines()
    if lines[0] != "def run(value: i64) -> i64:":
        raise ValueError("only def run(value: i64) -> i64 is supported")
    operations = []
    result = None
    for raw in lines[1:]:
        if not raw.strip():
            continue
        if not raw.startswith("    ") or raw.startswith("        "):
            raise ValueError("exactly one indented suite is supported")
        line = raw.strip()
        mutable = line.startswith("mut ")
        if mutable:
            line = line[4:]
        try:
            node = ast.parse(without_borrows(line)).body[0]
        except SyntaxError as error:
            raise ValueError("unsupported statement syntax") from error
        if isinstance(node, ast.Assign) and result is None:
            if len(node.targets) != 1 or not isinstance(node.targets[0], ast.Name):
                raise ValueError("simple assignments only")
            call = node.value
            if not isinstance(call, ast.Call) or not isinstance(call.func, ast.Name):
                raise ValueError("binding must call a selected API")
            name = call.func.id
            if name not in BINDINGS or call.keywords or len(call.args) != len(BINDINGS[name][0]):
                raise ValueError("unsupported API/arity")
            args = []
            for arg in call.args:
                if isinstance(arg, ast.Name):
                    args.append(arg.id)
                elif isinstance(arg, ast.Constant) and isinstance(arg.value, str):
                    args.append({"text": arg.value})
                elif isinstance(arg, ast.Constant) and type(arg.value) is int:
                    args.append(arg.value)
                else:
                    raise ValueError("only names, integer and text arguments")
            operations.append({"bind": node.targets[0].id, "call": name, "args": args, "mutable": mutable})
        elif isinstance(node, ast.Expr) and result is None:
            def flatten(expr):
                if isinstance(expr, ast.Name):
                    return [expr.id]
                if isinstance(expr, ast.BinOp) and isinstance(expr.op, ast.Add):
                    return flatten(expr.left) + flatten(expr.right)
                raise ValueError("final expression must be a sum of names")
            result = flatten(node.value)
        else:
            raise ValueError("unsupported statement")
    if result is None:
        raise ValueError("missing result")
    return {"operations": operations, "sum": result}


def checked_source(source):
    prelude = "class Matcher:\n    token: i64\n\nclass Pending:\n    token: i64\n\n"
    for name, (params, ret) in BINDINGS.items():
        params = ", ".join(f"arg{i}: {ty}" for i, ty in enumerate(params))
        body = "0" if ret == "i64" else f"{ret}(0)"
        prelude += f"def {name}({params}) -> {ret}:\n    {body}\n\n"
    # Signature-only stubs are checked but never executed in any backend.
    return prelude + source + "\ndef main() -> i32:\n    result = run(41)\n    0i32\n"


def rust_text(text):
    # json's Unicode escape spelling differs from Rust's; keep Unicode literal.
    return json.dumps(text, ensure_ascii=False).replace("\\u0000", "\\0")


def rust_source(plan):
    lines = ["pub fn probe_run(value: i64) -> i64 {"]
    for operation in plan["operations"]:
        name = operation["call"]
        args = []
        for arg, ty in zip(operation["args"], BINDINGS[name][0]):
            value = rust_text(arg["text"]) if isinstance(arg, dict) else str(arg)
            args.append(("&mut " if ty.startswith("&mut ") else "&" if ty.startswith("&") else "") + value)
        generic = "::<i64>" if name == "identity" else ""
        mutable = "mut " if operation["mutable"] else ""
        lines.append(f"    let {mutable}{operation['bind']} = fixture::{name}{generic}({', '.join(args)});")
    # Preserve Plenty's trapping integer arithmetic in release Rust too.
    result = plan["sum"][0]
    for name in plan["sum"][1:]:
        result = f"({result}).checked_add({name}).unwrap()"
    lines += [f"    {result}", "}"]
    return "\n".join(lines)


def adapters(abi):
    lines = ["extern crate plenty_rust_crate_probe as fixture;", "pub fn keep_linked() {}"]
    for name, (params, ret) in BINDINGS.items():
        declarations, arguments = [], []
        for index, ty in enumerate(params):
            var = f"arg{index}"
            if ty == "str":
                declarations += [f"{var}: *const u8", f"{var}_len: usize"]
                arguments.append(f"std::str::from_utf8(unsafe {{ std::slice::from_raw_parts({var}, {var}_len) }}).unwrap()")
            elif ty == "i64":
                declarations.append(f"{var}: i64")
                arguments.append(var)
            else:
                owner = ty.replace("&mut ", "").replace("&", "")
                declarations.append(f"{var}: *mut fixture::{owner}")
                if ty.startswith("&mut "):
                    arguments.append(f"unsafe {{ &mut *{var} }}")
                elif ty.startswith("&"):
                    arguments.append(f"unsafe {{ &*{var} }}")
                else:
                    arguments.append(f"unsafe {{ *Box::from_raw({var}) }}")
        generic = "::<i64>" if name == "identity" else ""
        call = f"fixture::{name}{generic}({', '.join(arguments)})"
        output = "i64" if ret == "i64" else f"*mut fixture::{ret}"
        if ret != "i64":
            call = f"Box::into_raw(Box::new({call}))"
        lines += ["/// # Safety", "/// Inputs satisfy the generated fixture's pointer, UTF-8 and ownership contract.",
                  "#[unsafe(no_mangle)]", f'pub unsafe extern "{abi}" fn bridge_{name}({", ".join(declarations)}) -> {output} {{', f"    {call}", "}"]
    return "\n".join(lines)


def harness(body, foreign):
    declaration = 'unsafe extern "C" { fn probe_run(value: i64) -> i64; }' if foreign else body
    invoke = "unsafe { probe_run(value) }" if foreign else "probe_run(value)"
    return f'''extern crate plenty_rust_crate_probe as fixture;
{body if foreign else ''}
{declaration}
fn main() {{
    {"adapters::keep_linked();" if foreign else ""}
    if std::env::args().any(|arg| arg == "overflow") {{
        let value = std::hint::black_box(i64::MAX / 2);
        std::hint::black_box({invoke});
        panic!("overflow incorrectly returned");
    }}
    for value in [-100i64, -1, 0, 1, 41, 1000] {{
        assert_eq!({invoke}, 3 * value + 11);
        assert_eq!(fixture::live_owners(), 0);
    }}
    let value = std::hint::black_box(41i64);
    let start = std::time::Instant::now();
    let mut checksum = 0i64;
    for _ in 0..2000 {{
        let value = std::hint::black_box(value);
        checksum += std::hint::black_box({invoke});
    }}
    println!("{{}} {{}}", start.elapsed().as_nanos(), checksum);
}}
'''


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    _, version, _ = command(["rustc", "--version"])
    if version.strip() != PIN:
        raise RuntimeError(f"Rust-ABI probe is qualified only for {PIN}; got {version.strip()}")
    _, full_version, _ = command(["rustc", "-vV"])
    if "host: x86_64-unknown-linux-gnu" not in full_version:
        raise RuntimeError("probe is qualified only for x86_64-unknown-linux-gnu")
    build, _, _ = command(["cargo", "build", "--locked", "--offline", "--target-dir", str(ROOT / "target")])
    tool = str(TARGET / "plenty-rust-crate-probe")
    source = (ROOT / "workload.plenty").read_text()
    plan = parse(source)
    (OUT / "checked.plenty").write_text(checked_source(source))
    command([tool, "check", str(OUT / "checked.plenty")])
    # Existing checker rejects use-after-move in the exact fixture surface.
    bad = source.replace("    mut future", "    illegal = match_count(&matcher, \"a\")\n    mut future")
    (OUT / "moved.plenty").write_text(checked_source(bad))
    failure = subprocess.run([tool, "check", str(OUT / "moved.plenty")], text=True, capture_output=True)
    if failure.returncode == 0 or "moved" not in failure.stderr:
        raise RuntimeError("expected real Plenty use-after-move rejection: " + failure.stderr)
    (OUT / "plan.json").write_text(json.dumps(plan, indent=2))
    emit, _, _ = command([tool, "emit", str(OUT / "plan.json"), str(OUT / "caller.o")])
    scalar_plan = {"operations": [{"bind": "answer", "call": "scalar", "args": ["value"]}], "sum": ["answer"]}
    (OUT / "scalar.json").write_text(json.dumps(scalar_plan))
    command([tool, "emit", str(OUT / "scalar.json"), str(OUT / "scalar.o")])
    dependency = ["--extern", f"plenty_rust_crate_probe={TARGET / 'libplenty_rust_crate_probe.rlib'}", "-L", f"dependency={TARGET / 'deps'}"]
    result = {"toolchain": full_version, "cargo_build_seconds": build, "cranelift_emit_seconds": emit, "paths": {}}
    for profile, optimization in [("debug", "0"), ("release", "3")]:
        for mode in ["Rust", "C", "source"]:
            label = f"{mode}-{profile}"
            common = ["rustc", "--edition=2024", "-C", "panic=abort", "-C", f"opt-level={optimization}", *dependency]
            elapsed = 0
            if mode != "source":
                adapter_path = OUT / f"adapters-{label}.rs"
                adapter_path.write_text(adapters(mode))
                library = OUT / f"libadapters_{mode}_{profile}.rlib"
                dt, _, _ = command([*common, "--crate-name", "adapters", "--crate-type=rlib", str(adapter_path), "-o", str(library)])
                elapsed += dt
                program = harness("extern crate adapters;", True)
                extra = ["--extern", f"adapters={library}", "-C", f"link-arg={OUT / 'caller.o'}"]
            else:
                program = harness(rust_source(plan), False)
                extra = []
            path = OUT / f"{label}.rs"
            path.write_text(program)
            executable = OUT / label
            dt, _, _ = command([*common, "--crate-name", "probe", str(path), *extra, "-o", str(executable)])
            elapsed += dt
            overflow = subprocess.run([str(executable), "overflow"], text=True, capture_output=True)
            if overflow.returncode == 0 or "overflow incorrectly returned" in overflow.stderr:
                raise RuntimeError("expected arithmetic trap, not successful wraparound")
            samples = []
            for _ in range(3):
                _, output, _ = command([str(executable)])
                nanos, checksum = map(int, output.split())
                if checksum != 268000:
                    raise RuntimeError(f"checksum mismatch {checksum}")
                samples.append(nanos / 2000)
            result["paths"][label] = {"adapter_and_link_seconds": elapsed, "workload_ns_median": statistics.median(samples), "samples_ns": samples, "binary_bytes": executable.stat().st_size, "overflow_exit": overflow.returncode}
            if profile == "release":
                if mode == "source":
                    declaration = "fn probe_run(value: i64) -> i64 { fixture::scalar(value) }"
                    call = "probe_run(value)"
                    keep = ""
                    scalar_extra = []
                else:
                    declaration = 'extern crate adapters; unsafe extern "C" { fn probe_run(value: i64) -> i64; }'
                    call = "unsafe { probe_run(value) }"
                    keep = "adapters::keep_linked();"
                    scalar_extra = ["--extern", f"adapters={library}", "-C", f"link-arg={OUT / 'scalar.o'}"]
                scalar_source = f'''extern crate plenty_rust_crate_probe as fixture;
{declaration}
fn main() {{
    {keep}
    let value = std::hint::black_box(41i64);
    let start = std::time::Instant::now();
    let mut sum = 0i64;
    for _ in 0..2_000_000 {{
        let value = std::hint::black_box(value);
        sum += std::hint::black_box({call});
    }}
    assert_eq!(sum, 84_000_000);
    println!("{{}}", start.elapsed().as_nanos());
}}
'''
                path = OUT / f"scalar-{mode}.rs"
                path.write_text(scalar_source)
                executable = OUT / f"scalar-{mode}"
                command([*common, "--crate-name", "scalar_probe", str(path), *scalar_extra, "-o", str(executable)])
                samples = [int(command([str(executable)])[1]) / 2_000_000 for _ in range(3)]
                result["paths"][label]["scalar_ns_median"] = statistics.median(samples)
                result["paths"][label]["scalar_samples_ns"] = samples
    # Preserve emitted LLVM signatures: observed lowering, not a compiler ABI API.
    ir = OUT / "adapters.ll"
    command(["rustc", "--edition=2024", "--crate-name", "adapters", "--crate-type=rlib", "--emit=llvm-ir", "-C", "panic=abort", *dependency, str(OUT / "adapters-Rust-release.rs"), "-o", str(ir)])
    result["rust_abi_definitions"] = [line for line in ir.read_text().splitlines() if line.startswith("define ") and "@bridge_" in line]
    # This attempts the compiler-private entry point; absence is an explicit result.
    helper = subprocess.run(["rustc", "+nightly-2026-10-01", "--crate-type=lib", "-", "-o", str(OUT / "helper.rlib")], input="#![feature(rustc_private)]\nextern crate rustc_driver;\n", text=True, capture_output=True)
    result["rustc_private_probe"] = {"exit": helper.returncode, "diagnostic": helper.stderr}
    (OUT / "results.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
