#!/usr/bin/env python3
"""Materialize and validate checked-default acceptance inputs, without measuring.

Generate never invokes a compiler. Validate requires a pinned, hashed, frozen
binary manifest and records correctness only (even for mismatched debug builds).
Measure deliberately refuses execution: final pins, quiet-machine approval and
reviewed timing/inspection instrumentation are still required. No Cargo commands
or existing benchmark templates are changed or executed by this controller.
"""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import random
import re
import subprocess
import textwrap

MODES = ("T", "C", "I", "U")
OVERFLOW = "OverflowError[i64]"
MAX = (1 << 63) - 1
MIN = -(1 << 63)
TEXT = "alpha beta gamma delta " * 4096
TEXT_OUTPUT = ("alpha beta\ngamma\ndelta\n" * 4096)[:-1]
REPO = Path(__file__).resolve().parent.parent
# Pin the complete historical generator, including its entrypoint, for an
# auditable original-versus-adapted comparison rather than silent template drift.
COMPILE_BENCH_SHA256 = "14de3ba25e3c4feaea9120d9decebf4d039891c73cbc51c9d3594dfc07b56b73"
REUSED_TESTS = [
    "test_arithmetic_errors::overflow_carries_the_wrapped_value_for_every_width",
    "test_arithmetic_errors::division_floors_and_reports_zero_divisors_and_the_one_overflowing_quotient",
    "test_arithmetic_errors::a_negative_exponent_still_traps",
    "test_arithmetic_errors::compound_assignment_and_forwarded_operators_are_checked",
    "test_checked_defaults::implicit_failures_release_locals_and_pending_temporaries_without_writing",
    "test_checked_defaults::generators_require_local_handling_or_explicit_traps",
]
NOT_IMPLEMENTED = [
    "runtime and compiler timing execution (including internal timer regions)",
    "profiling, Rust build-cost measurement and allocator totals",
    "IR/ABI/guard/object-size extraction and optimized-object library helper",
    "full arithmetic/divisor/pow/callback/ownership matrix (reuse existing named tests)",
]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def fresh_directory(path):
    path = Path(path)
    path.mkdir(parents=True, exist_ok=False)
    return path.resolve()


def wrap(value, bits=64, signed=True):
    value %= 1 << bits
    return value - (1 << bits) if signed and value >= (1 << (bits - 1)) else value


def schedule(warmups=5, samples=30):
    """Balanced cyclic orders with deterministic shuffled blocks; never drop rows."""
    rng = random.Random(88616)
    rows = []
    for phase, count in (("warmup", warmups), ("sample", samples)):
        for block in range((count + 3) // 4):
            order = list(MODES)
            rng.shuffle(order)
            if block % 2:
                order.reverse()
            for rotation in range(min(4, count - block * 4)):
                for mode in order[rotation:] + order[:rotation]:
                    rows.append({"phase": phase, "round": block * 4 + rotation, "mode": mode})
    return rows


def function(mode, name, parameters, result, errors, statements, value, public=False):
    """Every helper owns its lexical mode; C yields a value, I writes Ok."""
    recoverable = mode in ("C", "I")
    output = f"Result[{result}, {errors}]" if recoverable or public else result
    tail = value if mode == "C" or not (recoverable or public) else f"Ok({value})"
    body = statements + tail + "\n"
    if mode in ("C", "U"):
        body = ("checked:\n" if mode == "C" else "unchecked:\n") + textwrap.indent(body, "    ")
    return f"def {name}({parameters}) -> {output}:\n" + textwrap.indent(body, "    ") + "\n"


def report(result_type="i64", error_type=OVERFLOW):
    return f"""def report(result: Result[{result_type}, {error_type}]) -> Result[(), IoError]:
    match result:
        case Ok(value):
            print("ok", value)?
        case Err(error):
            print("overflow", error.wrapped)?
    Ok(())

"""


def parsed_main(mode, name, arguments):
    setup = "    options = args()?\n"
    for i, argument in enumerate(arguments, 1):
        setup += f"    {argument} = i64.parse(&options[{i}])?\n"
    call = f"{name}({', '.join(arguments)})"
    output = f"report({call})?" if mode in ("C", "I") else f'print("ok", {call})?'
    return ("def main() -> Result[(), AllocError | ParseError | IoError]:\n" + setup
            + f'    {output}\n    print("after")?\n    Ok(())\n')


def tight_source(mode):
    body = "mut total = start\nfor _ in range(n):\n    total += step\n"
    return ((report() if mode in ("C", "I") else "")
            + function(mode, "kernel", "n: i64, start: i64, step: i64", "i64", OVERFLOW, body, "total")
            + parsed_main(mode, "kernel", ("n", "start", "step")))


def calls_source(mode):
    leaf = function(mode, "leaf", "value: i64, factor: i64, bias: i64", "i64", OVERFLOW,
                    "", "value * factor + bias")
    propagate = "?" if mode in ("C", "I") else ""
    body = f"mut total = 0\nfor i in range(n):\n    total += leaf(base + i, factor, bias){propagate}\n"
    return ((report() if propagate else "") + leaf
            + function(mode, "calls", "n: i64, base: i64, factor: i64, bias: i64",
                       "i64", OVERFLOW, body, "total")
            + parsed_main(mode, "calls", ("n", "base", "factor", "bias")))


def recovery_source(mode):
    return (function(mode, "leaf", "value: i64, factor: i64, bias: i64", "i64", OVERFLOW,
                     "", "value * factor + bias") + """def main() -> Result[(), AllocError | ParseError | IoError]:
    options = args()?
    repetitions = i64.parse(&options[1])?
    mut errors = 0
    mut checksum = 0
    for _ in range(repetitions):
        match leaf(i64.MAX, 2, 1):
            case Ok(_):
                print("unexpected-ok")?
            case Err(error):
                errors = errors.wrapping_add(1)
                checksum = checksum.wrapping_add(error.wrapped)
    print(errors, checksum)?
    Ok(())
""")


def text_source(mode):
    body = ("mut total = 0\nfor _ in range(repetitions):\n"
            "    result = std.text.fill(text, 10, output_limit, input_limit)?\n"
            "    total += result.byte_len()\n")
    return ("import std.text\n\n"
            + function(mode, "consume", "text: &str, repetitions: i64, output_limit: NonNegative[i64], input_limit: NonNegative[i64]",
                       "i64", f"std.text.TextError | {OVERFLOW}", body, "total", public=True)
            + """def main() -> Result[(), AllocError | ParseError | RangeError | IoError | std.text.TextError]:
    options = args()?
    text = "alpha beta gamma delta ".repeat(4096)?
    if options[1] == "output":
        print(std.text.fill(&text, 10, 8388608, 8388608)?)?
        return Ok(())
    repetitions = i64.parse(&options[1])?
    output_limit = NonNegative.try_from(i64.parse(&options[2])?)?
    input_limit = NonNegative.try_from(i64.parse(&options[3])?)?
    match consume(&text, repetitions, output_limit, input_limit):
        case Ok(value):
            print("ok", value)?
        case Err(error):
            match error:
                case overflow: OverflowError[i64]:
                    print("overflow", overflow.wrapped)?
                case fault: std.text.TextError:
                    match fault:
                        case std.text.TextError.LimitExceeded:
                            print("text-limit")?
                        case _:
                            print("unexpected-text-error")?
    print("after")?
    Ok(())
""")


def width_source(mode, ty):
    error = f"OverflowError[{ty}]"
    call = "add(a, b)"
    output = f"report({call})?" if mode in ("C", "I") else f'print("ok", {call})?'
    return ((report(ty, error) if mode in ("C", "I") else "")
            + function(mode, "add", f"a: {ty}, b: {ty}", ty, error, "", "a + b")
            + f"""def main() -> Result[(), AllocError | ParseError | IoError]:
    options = args()?
    a = {ty}.parse(&options[1])?
    b = {ty}.parse(&options[2])?
    {output}
    print("after")?
    Ok(())
""")


def cleanup_source(mode):
    body = "owner = Owner(10)\nitems = [1, 2, 3]?\n*n += i64.MAX\ndrop(owner)\ndrop(items)\n"
    return ("""class Owner:
    id: i64
    def __del__(self: &mut Owner) -> ():
        print("drop", self.id).unwrap()

""" + function(mode, "cleanup", "n: &mut i64", "()", f"AllocError | {OVERFLOW}", body, "()", public=True)
            + """def main() -> Result[(), AllocError | ParseError | IoError]:
    options = args()?
    mut n = i64.parse(&options[1])?
    match cleanup(&mut n):
        case Ok(_):
            print("ok")?
        case Err(error):
            match error:
                case overflow: OverflowError[i64]:
                    print("overflow", overflow.wrapped)?
                case allocation: AllocError:
                    print("unexpected-allocation-error")?
    print("after", n)?
    Ok(())
""")


def allocation_source(mode, positive=False):
    if positive:
        return """def main() -> ():
    print("__test_begin_no_allocations__").unwrap()
    values = [1, 2, 3].unwrap()
    drop(values)
    print("__test_end_no_allocations__").unwrap()
"""
    return (function(mode, "step", "a: i64, b: i64", "i64", f"{OVERFLOW} | ZeroDivisionError",
                     "", "a * 2 // b") + """def main() -> Result[(), IoError]:
    print("__test_begin_no_allocations__").unwrap()
    good = step(1, 2)
    overflow = step(i64.MAX, 0)
    zero = step(1, 0)
    print("__test_end_no_allocations__").unwrap()
    match good:
        case Ok(value):
            print("ok", value)?
        case Err(_):
            print("unexpected-good-error")?
    match overflow:
        case Err(OverflowError(wrapped)):
            print("overflow", wrapped)?
        case _:
            print("unexpected-overflow")?
    match zero:
        case Err(ZeroDivisionError()):
            print("zero")?
        case _:
            print("unexpected-zero")?
    Ok(())
""")


def original_template(repo):
    rust = (repo / "examples/compile_bench.rs").read_text(encoding="utf-8")
    found = re.search(r'"(protocol Measured\{i\}:.*?)"\n        \)\);', rust, re.S)
    if digest(rust.encode()) != COMPILE_BENCH_SHA256 or not found or "\\" in found[1]:
        raise ValueError("compile_bench template changed; review extraction rather than replace workload")
    return rust, found[1]


def corpus_unit(mode, i):
    recoverable = mode in ("C", "I")
    result = f"Result[i64, {OVERFLOW}]" if recoverable else "i64"
    protocol = f"protocol Measured{i}:\n    def squared_length(self) -> {result}:\n        pass\n\n"
    length = function(mode, "squared_length", "self", "i64", OVERFLOW, "",
                      f"self.x * self.x + self.y * self.y + {i}")
    shift = function(mode, "shift", f"self: &mut Point{i}, amount: i64", "()", OVERFLOW,
                     "self.x = self.x + amount\nself.y = self.y + amount\n", "()")
    unit = protocol + f"class Point{i}:\n    x: i64\n    y: i64\n\n" + textwrap.indent(length + shift, "    ")
    unit += f"enum Reading{i}:\n    Missing\n    Value(i64)\n\n"
    match = f"match reading:\n    case Reading{i}.Missing:\n        0\n    case Reading{i}.Value(value):\n        value + {i}\n"
    if mode == "I":
        match = match.replace("        0\n", "        Ok(0)\n").replace(f"        value + {i}\n", f"        Ok(value + {i})\n")
    if mode in ("C", "U"):
        match = ("checked:\n" if mode == "C" else "unchecked:\n") + textwrap.indent(match, "    ")
    unit += f"def describe{i}(reading: Reading{i}) -> {result}:\n" + textwrap.indent(match, "    ") + "\n"
    comprehension = f"[n * n + {i} for n in range(limit) if n % 2 == 0]"
    error = f"AllocError | {OVERFLOW} | ZeroDivisionError" if recoverable else "AllocError"
    expression = f"checked:\n    {comprehension}?" if mode == "C" else (f"Ok({comprehension}?)" if mode == "I" else f"unchecked:\n    {comprehension}")
    unit += f"def squares{i}(limit: i64) -> Result[list[i64], {error}]:\n" + textwrap.indent(expression, "    ") + "\n\n"
    q = "?" if recoverable else ""
    work = (f"mut point = Point{i}(a, {i})\npoint.shift(1){q}\nvalues = squares{i}(a)?\n"
            f"mut total = point.squared_length(){q} + describe{i}(Reading{i}.Value(a)){q}\n"
            f"for v in &values:\n    if v > {i}:\n        total = total + v\n")
    work_errors = f"AllocError | {OVERFLOW} | ZeroDivisionError" if recoverable else "Failure"
    return unit + function(mode, f"work{i}", "a: i64", "i64", work_errors, work, "total", public=True)


def corpus_source(mode, units, template):
    body = "".join(template.replace("{i}", str(i)) if mode == "T" else corpus_unit(mode, i)
                   for i in range(units))
    return body + f"def main() -> Result[(), Failure]:\n    print(work{units - 1}(5)?)?\n    Ok(())\n"


def case(name, args, stdout=None, trap=False, classification=None):
    return {"name": name, "args": [str(a) for a in args], "stdout": stdout,
            "expected": classification or ("arithmetic-trap" if trap else "success")}


def arithmetic_case(mode, name, args, value=None, wrapped=None):
    if wrapped is not None and mode in ("T", "U"):
        return case(name, args, trap=True)
    output = f"overflow {wrapped}\nafter\n" if wrapped is not None else f"ok {value}\nafter\n"
    return case(name, args, output, classification="handled-error" if wrapped is not None else None)


def generate(output, repo=REPO, units=(1000, 4000), allocations=False):
    if not units or any(type(count) is not int or count <= 0 for count in units) or len(set(units)) != len(units):
        raise ValueError("corpus unit counts must be distinct positive integers")
    rust, template = original_template(repo)
    root = fresh_directory(output)
    entries = []

    def add(mode, name, source, cases, allocation=False):
        path = root / "sources" / mode / (name + ".plenty")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source, encoding="utf-8")
        entries.append({"mode": mode, "source": str(path.relative_to(root)),
                        "sha256": digest(path.read_bytes()), "cases": cases,
                        "requires_runtime_checks": allocation})

    for mode in MODES:
        add(mode, "tight", tight_source(mode), [
            arithmetic_case(mode, "success", (10000000, 17, 3), value=30000017),
            arithmetic_case(mode, "empty", (0, 17, 3), value=17),
            arithmetic_case(mode, "boundary", (1, MAX - 1, 1), value=MAX),
            arithmetic_case(mode, "early", (1, MAX, 1), wrapped=MIN),
            arithmetic_case(mode, "late", (1000000, MAX - 999999, 1), wrapped=MIN)])
        add(mode, "calls", calls_source(mode), [
            arithmetic_case(mode, "success", (1000000, 1000000000, 3, 1), value=3001499999500000),
            arithmetic_case(mode, "first-multiply", (1, MAX, 2, 1), wrapped=-2),
            arithmetic_case(mode, "first-add", (1, MAX, 1, 1), wrapped=MIN),
            arithmetic_case(mode, "late-argument", (2, MAX, 1, 0), wrapped=MIN)])
        add(mode, "text", text_source(mode), [
            case("full-output", ("output",), TEXT_OUTPUT + "\n"),
            case("success", (100, 8388608, 8388608), "ok 9420700\nafter\n"),
            case("input-budget", (100, 8388608, 94207), "text-limit\nafter\n", classification="handled-error"),
            case("output-budget", (100, 94206, 8388608), "text-limit\nafter\n", classification="handled-error")])
        cleanup_cases = [case("success", (0,), f"drop 10\nok\nafter {MAX}\n")]
        cleanup_cases.append(case("skipped-write", (1,), f"drop 10\noverflow {MIN}\nafter 1\n", classification="handled-error")
                             if mode in ("C", "I") else case("skipped-write", (1,), trap=True))
        add(mode, "cleanup", cleanup_source(mode), cleanup_cases)
        for signed in (True, False):
            for bits in (8, 16, 32, 64):
                ty = ("i" if signed else "u") + str(bits)
                maximum = (1 << (bits - int(signed))) - 1
                add(mode, "add-" + ty, width_source(mode, ty), [
                    arithmetic_case(mode, "boundary", (maximum - 1, 1), value=maximum),
                    arithmetic_case(mode, "overflow", (maximum, 1), wrapped=wrap(maximum + 1, bits, signed))])
        for count in units:
            expected = (count - 1) ** 2 + 6 * (count - 1) + 62
            add(mode, f"corpus-{count}", corpus_source(mode, count, template), [case("checksum", (), f"{expected}\n")])
        if mode in ("C", "I"):
            add(mode, "recovered-errors", recovery_source(mode), [case("repeated-err", (1000,), "1000 -2000\n", classification="repeated-error")])
            if allocations:
                add(mode, "noalloc", allocation_source(mode), [case("arithmetic-errors", (),
                    "__test_begin_no_allocations__\n__test_end_no_allocations__\nok 1\noverflow -2\nzero\n")], True)
                add(mode, "allocation-control", allocation_source(mode, True),
                    [case("markers-active", (), classification="allocation-control")], True)
    (root / "original-template.txt").write_text(template, encoding="utf-8")
    inventory = {"schema": 1, "purpose": "unvalidated-inputs-not-acceptance", "entries": entries,
                 "template_sha256": digest(rust.encode()), "units": list(units),
                 "text_output_sha256": digest(TEXT_OUTPUT.encode()), "unimplemented": NOT_IMPLEMENTED,
                 "reused_tests_not_run": REUSED_TESTS,
                 "runtime_order": schedule(), "compiler_order": schedule(3, 15), "seed": 88616}
    write_json(root / "inventory.json", inventory)
    return inventory


BUILD_FIELDS = ("source_sha", "source_tree", "binary", "binary_sha256", "frozen_snapshot",
                "compiler_profile", "cargo_features", "cargo_flags", "rustflags", "runtime_rustflags",
                "target", "toolchain", "runtime_archive_sha256", "build_command", "native_linker")
MATCH_FIELDS = ("compiler_profile", "cargo_features", "cargo_flags", "rustflags", "runtime_rustflags",
                "target", "toolchain", "native_linker")


def load_manifest(path, measurement=False):
    manifest = json.loads(Path(path).read_text(encoding="utf-8"))
    if manifest.get("schema") != 1:
        raise ValueError("build manifest schema must be 1")
    if manifest.get("purpose") not in ("syntax-correctness-only", "measurement"):
        raise ValueError("build manifest needs an explicit purpose")
    for label in ("main", "candidate"):
        build = manifest["builds"][label]
        if any(field not in build for field in BUILD_FIELDS):
            raise ValueError(f"incomplete {label} build manifest; required: {BUILD_FIELDS}")
        if not re.fullmatch(r"[0-9a-f]{40}", build["source_sha"]):
            raise ValueError("source_sha must be a full commit hash")
        for key in ("binary_sha256", "runtime_archive_sha256"):
            if not re.fullmatch(r"[0-9a-f]{64}", build[key]):
                raise ValueError(f"{key} must be SHA256")
        if build["frozen_snapshot"] is not True:
            raise ValueError("a frozen binary snapshot is required, not an active Cargo output")
        if not isinstance(build["cargo_features"], list) or not isinstance(build["cargo_flags"], list):
            raise ValueError("features and Cargo flags must be lists")
        if not isinstance(build["build_command"], list) or not build["build_command"]:
            raise ValueError("build_command must preserve the explicit invocation")
        if not all(isinstance(build[key], str) and build[key] for key in
                   ("compiler_profile", "target", "toolchain", "native_linker")):
            raise ValueError("build profile, target, toolchain and linker must be explicit")
        binary, tree = Path(build["binary"]), Path(build["source_tree"])
        if not binary.is_absolute() or not tree.is_absolute():
            raise ValueError("binary and source_tree must be absolute")
        if digest(binary.read_bytes()) != build["binary_sha256"]:
            raise ValueError(f"{label} binary hash mismatch")
        head = subprocess.check_output(["git", "-C", str(tree), "rev-parse", "HEAD"], text=True).strip()
        if head != build["source_sha"]:
            raise ValueError(f"{label} source pin mismatch")
        subprocess.run(["git", "-C", str(tree), "diff", "--quiet", "HEAD", "--", "src", "stdlib",
                        "plenty-runtime", "Cargo.toml", "Cargo.lock", "build.rs", ".cargo"], check=True)
    if measurement:
        if manifest["purpose"] != "measurement" or manifest.get("quiet_machine_approved") is not True:
            raise ValueError("measurement needs purpose=measurement and explicit quiet-machine approval")
        before, after = (manifest["builds"][name] for name in ("main", "candidate"))
        if any(before[key] != after[key] for key in MATCH_FIELDS):
            raise ValueError("measurement build settings do not match")
    return manifest


def load_inventory(root):
    root = Path(root).resolve()
    inventory = json.loads((root / "inventory.json").read_text(encoding="utf-8"))
    if inventory.get("schema") != 1 or not inventory.get("entries"):
        raise ValueError("invalid source inventory")
    seen_sources = set()
    seen_outputs = set()
    for entry in inventory["entries"]:
        source = root / entry["source"]
        resolved = source.resolve()
        if entry["mode"] not in MODES or not resolved.is_relative_to(root):
            raise ValueError("source path/mode escapes input inventory")
        output_identity = (entry["mode"], source.stem)
        if resolved in seen_sources:
            raise ValueError("duplicate resolved source")
        if output_identity in seen_outputs:
            raise ValueError("duplicate executable output identity")
        if digest(resolved.read_bytes()) != entry["sha256"]:
            raise ValueError("changed source")
        seen_sources.add(resolved)
        seen_outputs.add(output_identity)
    return inventory


def classify(row, expected, source):
    code, stdout, stderr = row["returncode"], row["stdout"], row["stderr"]
    if row.get("spawn_error"):
        return "spawn-error"
    if row.get("timeout"):
        return "timeout"
    if code is not None and code < 0:
        return "unexpected-signal"
    if "panicked" in stderr or "assertion failed" in stderr:
        return "unexpected-panic"
    if expected["expected"] == "arithmetic-trap":
        if (code == 1 and "integer overflow" in stderr and Path(source).name + ":" in stderr
                and stderr.startswith("error:") and "after" not in stdout):
            return "expected-arithmetic-trap"
    elif expected["expected"] == "allocation-control":
        if code == 1 and stderr == "error: unexpected successful allocation in allocation-free region\n":
            return "expected-allocation-control"
    elif (expected["expected"] in ("success", "handled-error", "repeated-error")
          and code == 0 and stdout == expected["stdout"] and stderr == ""):
        return "expected-" + expected["expected"]
    return "wrong-outcome"


def execute(command, env, timeout):
    """No clock, retry or warmup here; retain raw streams even on timeout."""
    spawn_error = None
    try:
        result = subprocess.run(command, env=env, capture_output=True, timeout=timeout)
        code, out, err, expired = result.returncode, result.stdout, result.stderr, False
    except subprocess.TimeoutExpired as error:
        code, out, err, expired = None, error.stdout or b"", error.stderr or b"", True
    except OSError as error:
        code, out, err, expired = None, b"", str(error).encode(), False
        spawn_error = str(error)
    return {"command": command, "returncode": code, "timeout": expired, "spawn_error": spawn_error,
            "stdout": out.decode("utf-8", errors="replace"), "stderr": err.decode("utf-8", errors="replace"),
            "stdout_base64": base64.b64encode(out).decode(), "stderr_base64": base64.b64encode(err).decode()}


def validate(inputs, manifest_path, output, dry_run=False, timeout=120, families=None, modes=None):
    if timeout <= 0:
        raise ValueError("timeout must be positive")
    inputs = Path(inputs).resolve()
    inventory = load_inventory(inputs)
    manifest = load_manifest(manifest_path)
    available = {Path(entry["source"]).stem for entry in inventory["entries"]}
    if families and set(families) - available:
        raise ValueError("unknown source families: " + ", ".join(sorted(set(families) - available)))
    if modes is not None and (not modes or set(modes) - set(MODES)):
        raise ValueError("selected modes must be a nonempty subset of T, C, I, U")
    entries = [entry for entry in inventory["entries"]
               if (not families or Path(entry["source"]).stem in families)
               and (modes is None or entry["mode"] in modes)]
    if not entries:
        raise ValueError("no sources selected")
    for entry in entries:
        label = "main" if entry["mode"] in ("T", "C") else "candidate"
        if entry["requires_runtime_checks"] and "runtime-checks" not in manifest["builds"][label]["cargo_features"]:
            raise ValueError("allocation probes require an actual runtime-checks build manifest")
    root = fresh_directory(output)
    write_json(root / "build-manifest.json", manifest)
    write_json(root / "input-inventory.json", inventory)
    write_json(root / "validation.json", {"purpose": "syntax-correctness-only-not-performance", "dry_run": dry_run,
               "inputs": str(inputs), "inventory_sha256": digest((inputs / "inventory.json").read_bytes()),
               "selected_modes": [mode for mode in MODES if any(entry["mode"] == mode for entry in entries)],
               "selected_sources": [entry["source"] for entry in entries], "unimplemented": NOT_IMPLEMENTED})
    passed = True
    env = dict(os.environ, PLENTY_CACHE="off", PLENTY_CACHE_DIR=str(root / "cache"))
    for key in list(env):
        if key.startswith("HOTPATH_") or key == "PLENTY_PROFILE":
            env.pop(key)
    write_json(root / "environment.json", {key: env.get(key) for key in
               ("PLENTY_CACHE", "PLENTY_CACHE_DIR", "PLENTY_LINKER_THREADS", "COMPILER_PATH", "PATH")})
    with (root / "results.jsonl").open("x", encoding="utf-8") as log:
        def record(row):
            log.write(json.dumps(row, sort_keys=True) + "\n")
            log.flush()

        for entry in entries:
            label = "main" if entry["mode"] in ("T", "C") else "candidate"
            binary = manifest["builds"][label]["binary"]
            source = str(inputs / entry["source"])
            for profile in ("debug", "release"):
                executable = root / entry["mode"] / profile / Path(source).stem
                executable.parent.mkdir(parents=True, exist_ok=True)
                commands = [("check", [binary, "--check", source]),
                            ("build", [binary] + (["--release"] if profile == "release" else [])
                             + ["--no-cache", "--no-strip", "--compile", source, "-o", str(executable)])]
                ready = True
                for phase, command in commands:
                    row = {"mode": entry["mode"], "source": entry["source"], "profile": profile, "phase": phase}
                    if dry_run:
                        record(dict(row, command=command, classification="not-run"))
                        continue
                    result = execute(command, env, timeout)
                    okay = result["returncode"] == 0 and not result["timeout"]
                    record(dict(row, **result, classification="passed" if okay else phase + "-failed"))
                    if not okay:
                        passed, ready = False, False
                        break  # A rejected compile is never a fast runtime sample.
                if not ready:
                    continue
                for expected in entry["cases"]:
                    command = [str(executable), *expected["args"]]
                    row = {"mode": entry["mode"], "source": entry["source"], "profile": profile,
                           "phase": "runtime", "case": expected["name"]}
                    if dry_run:
                        record(dict(row, command=command, classification="not-run"))
                        continue
                    result = execute(command, env, timeout)
                    outcome = classify(result, expected, source)
                    passed &= outcome.startswith("expected-")
                    record(dict(row, **result, classification=outcome))
    write_json(root / "result.json", {"passed": passed if not dry_run else None, "dry_run": dry_run,
                                      "performance_evidence": False})
    return passed


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    phases = parser.add_subparsers(dest="phase", required=True)
    gen = phases.add_parser("generate", help="new directory only; no compiler execution")
    gen.add_argument("--output", required=True, type=Path)
    gen.add_argument("--allocation-probes", action="store_true")
    check = phases.add_parser("validate", help="untimed correctness, both generated-code profiles")
    check.add_argument("--inputs", required=True, type=Path)
    check.add_argument("--build-manifest", required=True, type=Path)
    check.add_argument("--output", required=True, type=Path)
    check.add_argument("--dry-run", action="store_true")
    check.add_argument("--timeout", type=int, default=120)
    check.add_argument("--family", action="append", help="exact source stem; repeat to select several")
    check.add_argument("--mode", choices=MODES, action="append", help="validate only selected modes; repeat to select several")
    measure = phases.add_parser("measure", help="reserved; explicitly unavailable in this MVP")
    measure.add_argument("--allow-measurements", action="store_true")
    measure.add_argument("--build-manifest", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        if args.phase == "generate":
            generate(args.output, allocations=args.allocation_probes)
        elif args.phase == "validate":
            return 0 if validate(args.inputs, args.build_manifest, args.output, args.dry_run,
                                 args.timeout, args.family, args.mode) else 1
        else:
            if not args.allow_measurements:
                raise ValueError("measurement requires --allow-measurements; no timings performed")
            load_manifest(args.build_manifest, measurement=True)
            raise ValueError("measurement execution not implemented; final pins and reviewed quiet-run harness required")
    except (ValueError, OSError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        parser.exit(2, f"error: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
