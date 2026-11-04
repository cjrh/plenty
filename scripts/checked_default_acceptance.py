#!/usr/bin/env python3
"""Generate, qualify and explicitly authorize checked-default measurements.

Generate/plan never invoke native tools. Validate records correctness only.
Measure requires --allow-perf, matching frozen release builds, an inventory hash
and explicit quiet-machine approval. Runtime timings are whole-process costs,
NOT kernel-only timings. No Cargo or profiler builds/commands are performed.
"""

import argparse
import base64
import hashlib
import json
import math
import os
import platform
import signal
import statistics
import struct
import time
from pathlib import Path
import random
import re
import subprocess
import textwrap

try:
    import resource
except ImportError:  # Generation/planning remain usable off POSIX.
    resource = None

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
    "internal kernel timing regions; runtime costs include startup/input/printing/teardown",
    "compiler phase/allocation profiling execution/parser blocked: separate count and approximate-byte reports required",
    "Rust compiler build-cost measurement and absolute native allocator totals",
    "optimized application object bytes (optimized ELF is not an application object)",
    "manual guard/ABI/cleanup audit and broader full-suite/native acceptance evidence",
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


def affine_leaf(mode):
    # A live product is the first stage of this affine transform. Its local
    # also excludes scalar leaf substitution; native call retention is gated
    # separately, never inferred from this source shape or compiler policy.
    return function(mode, "leaf", "value: i64, factor: i64, bias: i64", "i64", OVERFLOW,
                    "product = value * factor\n", "product + bias")


def calls_source(mode):
    leaf = affine_leaf(mode)
    propagate = "?" if mode in ("C", "I") else ""
    body = f"mut total = 0\nfor i in range(n):\n    total += leaf(base + i, factor, bias){propagate}\n"
    return ((report() if propagate else "") + leaf
            + function(mode, "calls", "n: i64, base: i64, factor: i64, bias: i64",
                       "i64", OVERFLOW, body, "total")
            + parsed_main(mode, "calls", ("n", "base", "factor", "bias")))


def recovery_source(mode):
    return (affine_leaf(mode) + """def main() -> Result[(), AllocError | ParseError | IoError]:
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
            + function(mode, "cleanup_after_mutation", "n: &mut i64", "()", f"AllocError | {OVERFLOW}",
                       body.replace("*n += i64.MAX", "*n += 1\n*n += i64.MAX"), "()", public=True)
            + """def main() -> Result[(), AllocError | ParseError | IoError]:
    options = args()?
    mut n = i64.parse(&options[1])?
    result = if len(options) > 2 and options[2] == "no-rollback":
        cleanup_after_mutation(&mut n)
    else:
        cleanup(&mut n)
    match result:
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
        cleanup_cases.append(case("earlier-mutation-retained", (0, "no-rollback"),
                                  f"drop 10\noverflow {MIN}\nafter 1\n", classification="handled-error")
                             if mode in ("C", "I") else case("earlier-mutation-retained", (0, "no-rollback"), trap=True))
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


def role_value(value, build):
    """Normalize owned path prefixes, not substrings; retain argv token boundaries."""
    paths = [(key, build[key]) for key in ("source_tree", "target_dir") if build.get(key) is not None]
    if any(not isinstance(path, str) or not Path(path).is_absolute() or not path.rstrip("/") for _, path in paths):
        raise ValueError("owned role paths must be absolute non-root paths")

    def path_token(token):
        # Only a path word or a supported explicit flag-value prefix.
        # '=' inside /external/key=/owned/root is not a flag boundary.
        prefix = re.match(r"(?:(?:--[A-Za-z0-9_-]+=)|(?:-L(?:(?:native|dependency|framework)=)?))?[\"']?", token)
        start = prefix.end()
        path_value = token[start:]
        if "\\" in token:  # Do not guess ownership through escaped spellings.
            return token
        if start and token[start - 1] in "\"'":
            if not path_value.endswith(token[start - 1]):
                return token
            path_value = path_value[:-1]
        if token.startswith("--remap-path-prefix="):
            path_value = path_value.split("=", 1)[0]
        if ".." in Path(path_value).parts:
            return token  # A lexical prefix can escape its purported owner.
        for key, path in sorted(paths, key=lambda item: len(item[1]), reverse=True):
            path = path.rstrip("/")
            if path_value == path or path_value.startswith(path + "/"):
                return token[:start] + "<" + key + ">" + token[start + len(path):]
        return token

    def normalize(part, argv_token=False):
        if isinstance(part, list):
            return [normalize(word, True) for word in part]
        if isinstance(part, str):
            if argv_token or part.startswith("/"):
                return path_token(part)
            # Preserve quoted groups and whitespace verbatim in flag strings.
            return re.sub(r'''(?:\\.|[^\s\\'\"]+|'[^']*'|"(?:\\.|[^"\\])*")+''', lambda match: path_token(match.group(0)), part)
        return part

    return normalize(value)


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
        if any(not isinstance(build[key], list) or not all(isinstance(part, str) for part in build[key])
               for key in ("cargo_features", "cargo_flags")):
            raise ValueError("features and Cargo flags must be string lists")
        if not isinstance(build["build_command"], list) or not build["build_command"] or not all(isinstance(part, str) for part in build["build_command"]):
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
        for build in (before, after):
            for key in ("rustflags", "runtime_rustflags"):
                value = build[key]
                if value is not None and not isinstance(value, str) and not (
                        isinstance(value, list) and all(isinstance(part, str) for part in value)):
                    raise ValueError("flag representations must be null, string or string list")
        if any(role_value(before[key], before) != role_value(after[key], after) for key in MATCH_FIELDS):
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
    if row.get("artifact_integrity_error"):
        return "integrity-error"
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


def retained_calls(disassembly, caller, callee="__plenty_fn_leaf"):
    """Count actual direct call instructions in the requested native function.

    Symbol declarations, comments, tail jumps and offset/interior targets are
    not evidence. Unrecognized tool output fails closed, not a guessed pass.
    """
    active = False
    sites = []
    for line in disassembly.splitlines():
        header = re.fullmatch(r"[0-9a-fA-F]+ <([^>]+)>:", line.strip())
        if header:
            active = header[1] == caller
        elif active and re.fullmatch(r"\s*[0-9a-fA-F]+:\s+(?:callq?|bl)\s+(?:0x)?[0-9a-fA-F]+\s+<"
                                     + re.escape(callee) + r">\s*", line):
            sites.append(line)
    return sites


def validate(inputs, manifest_path, output, dry_run=False, timeout=120, families=None, modes=None, objdump="objdump", runner=None, child_environment=None):
    run = runner or execute
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
               "selected_sources": [entry["source"] for entry in entries], "objdump": objdump,
               "unimplemented": NOT_IMPLEMENTED})
    passed = True
    env = dict(os.environ if child_environment is None else child_environment,
               PLENTY_CACHE="off", PLENTY_CACHE_DIR=str(root / "cache"))
    for key in list(env):
        if key.startswith(("HOTPATH_", "PLENTY_PROFILE")):
            env.pop(key)
    write_json(root / "environment.json", {key: env.get(key) for key in
               ("PLENTY_CACHE", "PLENTY_CACHE_DIR", "PLENTY_LINKER_THREADS", "COMPILER_PATH", "PATH", "LC_ALL")})
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
                    result = run(command, env, timeout)
                    okay = result["returncode"] == 0 and not result["timeout"] and not result.get("artifact_integrity_error")
                    record(dict(row, **result, classification="passed" if okay else phase + "-failed"))
                    if not okay:
                        passed, ready = False, False
                        break  # A rejected compile is never a fast runtime sample.
                if not ready:
                    continue
                family = Path(source).stem
                if profile == "release" and family in ("calls", "recovered-errors"):
                    caller = "__plenty_fn_calls" if family == "calls" else "__plenty_fn_main"
                    command = [objdump, "-drw", "--no-show-raw-insn", "--disassemble=" + caller, str(executable)]
                    row = {"mode": entry["mode"], "source": entry["source"], "profile": profile,
                           "phase": "retention", "caller": caller, "callee": "__plenty_fn_leaf"}
                    if dry_run:
                        record(dict(row, command=command, classification="not-run"))
                    else:
                        result = run(command, env, timeout)
                        sites = retained_calls(result["stdout"], caller)
                        qualified = result["returncode"] == 0 and not result["timeout"] and len(sites) == 1
                        record(dict(row, **result, static_call_sites=sites,
                                    executable_sha256=digest(executable.read_bytes()) if executable.is_file() else None,
                                    classification="retained-call" if qualified else "retention-blocked"))
                        if not qualified:
                            passed = False
                            continue  # Never qualify optimized call-heavy cases from values alone.
                for expected in entry["cases"]:
                    command = [str(executable), *expected["args"]]
                    row = {"mode": entry["mode"], "source": entry["source"], "profile": profile,
                           "phase": "runtime", "case": expected["name"]}
                    if dry_run:
                        record(dict(row, command=command, classification="not-run"))
                        continue
                    result = run(command, env, timeout)
                    outcome = classify(result, expected, source)
                    passed &= outcome.startswith("expected-")
                    record(dict(row, **result, classification=outcome))
    write_json(root / "result.json", {"passed": passed if not dry_run else None, "dry_run": dry_run,
                                      "performance_evidence": False})
    return passed


MEASURE_PHASES = ("runtime", "compiler", "code", "allocation")
RUNTIME_FAMILIES = ("tight", "calls", "text", "recovered-errors")


def pinned_inventory(inputs, expected_hash):
    root = Path(inputs).resolve()
    if not re.fullmatch(r"[0-9a-f]{64}", expected_hash or ""):
        raise ValueError("an explicit inventory SHA256 is required")
    if digest((root / "inventory.json").read_bytes()) != expected_hash:
        raise ValueError("inventory hash mismatch")
    return load_inventory(root)


def measurement_plan(inputs, inventory_sha256, phases=None, families=None,
                     runtime_warmups=5, runtime_samples=30, compiler_warmups=3, compiler_samples=15):
    inventory = pinned_inventory(inputs, inventory_sha256)
    phases = list(MEASURE_PHASES if phases is None else phases)
    if not phases or len(set(phases)) != len(phases) or set(phases) - set(MEASURE_PHASES):
        raise ValueError("invalid or duplicate measurement phases")
    counts = (runtime_warmups, runtime_samples, compiler_warmups, compiler_samples)
    if any(type(count) is not int or count < 0 for count in counts) or not runtime_samples or not compiler_samples:
        raise ValueError("sample counts must be positive and warmups nonnegative")
    families = list(RUNTIME_FAMILIES if families is None else families)
    if not families or len(set(families)) != len(families) or set(families) - set(RUNTIME_FAMILIES):
        raise ValueError("measurement runtime families must be tight, calls, text or recovered-errors")
    entries = inventory["entries"]
    if set(phases) & {"compiler", "code"}:
        if inventory["units"] != [1000, 4000]:
            raise ValueError("compiler/code phases require the complete 1000/4000 corpus")
        for count in (1000, 4000):
            if {e["mode"] for e in entries if Path(e["source"]).stem == f"corpus-{count}"} != set(MODES):
                raise ValueError("full four-mode compiler corpus is missing")
    selected = [e for e in entries if
                (set(phases) & {"runtime", "code"} and Path(e["source"]).stem in families) or
                (set(phases) & {"compiler", "code"} and Path(e["source"]).stem in ("corpus-1000", "corpus-4000"))]
    for family in families:
        if set(phases) & {"runtime", "code"}:
            modes = {e["mode"] for e in selected if Path(e["source"]).stem == family}
            if modes != (set("CI") if family == "recovered-errors" else set(MODES)):
                raise ValueError("incomplete runtime mode matrix")
    jobs = []
    if "runtime" in phases:
        for family in families:
            group = [e for e in selected if Path(e["source"]).stem == family]
            names = list(dict.fromkeys(c["name"] for e in group for c in e["cases"]))
            for name in names:
                for slot in schedule(runtime_warmups, runtime_samples):
                    entry = next((e for e in group if e["mode"] == slot["mode"]), None)
                    if entry is not None:
                        expected = next(c for c in entry["cases"] if c["name"] == name)
                        semantics = ("trap-process" if expected["expected"] == "arithmetic-trap" else
                                     "api-error-process" if family == "text" and expected["expected"] == "handled-error" else
                                     expected["expected"] + "-process")
                        jobs.append(dict(slot, stage="runtime", family=family, case=name, semantics=semantics,
                                         source=entry["source"], action="run"))
    if "compiler" in phases:
        for count in (1000, 4000):
            for action in ("check", "object-debug", "aot-debug", "aot-release"):
                for slot in schedule(compiler_warmups, compiler_samples):
                    entry = next(e for e in selected if e["mode"] == slot["mode"] and
                                 Path(e["source"]).stem == f"corpus-{count}")
                    jobs.append(dict(slot, stage="compiler", family=f"corpus-{count}", case=action,
                                     semantics="compiler-success", source=entry["source"], action=action))
    for ordinal, job in enumerate(jobs):
        job["id"] = f"job-{ordinal:06}"
    return {"schema": 1, "seed": 88616, "inventory_sha256": inventory_sha256,
            "phases": phases, "selected_sources": [e["source"] for e in selected], "jobs": jobs,
            "runtime_counts": [runtime_warmups, runtime_samples], "compiler_counts": [compiler_warmups, compiler_samples],
            "protocol": "planned-counts" if counts == (5, 30, 3, 15) and families == list(RUNTIME_FAMILIES) else "exploration-not-acceptance",
            "timing_scope": "whole-process: startup/parsing/input/printing/teardown included; no kernel-only attribution",
            "blocked_metrics": NOT_IMPLEMENTED}


def distribution(values):
    if not values or any(not math.isfinite(v) or v < 0 for v in values):
        raise ValueError("statistics require finite nonnegative values")
    ordered = sorted(values)
    def percentile(p):
        index = (len(ordered) - 1) * p
        lo = math.floor(index)
        return ordered[lo] + (ordered[math.ceil(index)] - ordered[lo]) * (index - lo)
    median = statistics.median(ordered)
    return {"n": len(values), "median": median, "p10": percentile(.1), "p90": percentile(.9),
            "min": ordered[0], "max": ordered[-1], "p90_minus_p10": percentile(.9) - percentile(.1),
            "stdev": statistics.stdev(values) if len(values) > 1 else 0,
            "mad": statistics.median(abs(value - median) for value in values)}


def sample_summary(rows):
    groups = {}
    for row in rows:
        key = (row["stage"], row["family"], row["case"], row["semantics"], row["mode"])
        groups.setdefault(key, []).append(row)
    summaries, complete = [], {}
    for key, group in groups.items():
        okay = all(row["status"] == "passed" for row in group)
        samples = [row for row in group if row["phase"] == "sample"]
        item = dict(zip(("stage", "family", "case", "semantics", "mode"), key),
                    status="complete" if okay and samples else "blocked", warmups=sum(r["phase"] == "warmup" for r in group),
                    samples=len(samples), failed_or_blocked=sum(r["status"] != "passed" for r in group))
        if okay and samples:
            item["wall_ns"] = distribution([r["elapsed_ns"] for r in samples])
            for metric in ("user_seconds", "sys_seconds", "peak_rss_kib"):
                if all(metric in r for r in samples):
                    item[metric] = distribution([r[metric] for r in samples])
            complete[key] = samples
        summaries.append(item)
    ratios = []
    for key, numerator in complete.items():
        for denominator_mode in MODES:
            if (key[-1], denominator_mode) not in (("U", "T"), ("I", "C"), ("C", "T"), ("I", "U")):
                continue
            other = (*key[:-1], denominator_mode)
            if other not in complete:
                continue  # Trap-process and recovered-error groups never share semantics.
            denominator = {row["round"]: row for row in complete[other]}
            if set(denominator) != {r["round"] for r in numerator} or any(r["elapsed_ns"] <= 0 for r in denominator.values()):
                continue
            ratios.append({"stage": key[0], "family": key[1], "case": key[2], "semantics": key[3],
                           "ratio": key[-1] + "/" + denominator_mode,
                           "paired_wall_ratio": distribution([r["elapsed_ns"] / denominator[r["round"]]["elapsed_ns"] for r in numerator])})
    return {"groups": summaries, "ratios": ratios}


def process_group(command, env, timeout, timed=False):
    """Direct child status, bounded descendants and lossless streams; serial only."""
    started_at = time.time_ns()
    usage = resource.getrusage(resource.RUSAGE_CHILDREN) if timed else None
    start = time.perf_counter_ns() if timed else None
    expired, spawn = False, None
    try:
        child = subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            out, err = child.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            expired = True
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            out, err = child.communicate()
        code = child.returncode
    except OSError as error:
        code, out, err, spawn = None, b"", str(error).encode(), str(error)
    elapsed = time.perf_counter_ns() - start if timed else None
    row = {"command": command, "started_at_unix_ns": started_at, "returncode": code,
           "timeout": expired, "spawn_error": spawn, "stdout": out.decode("utf-8", errors="replace"),
           "stderr": err.decode("utf-8", errors="replace"), "stdout_base64": base64.b64encode(out).decode(),
           "stderr_base64": base64.b64encode(err).decode()}
    if timed:
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        row.update(elapsed_ns=elapsed, user_seconds=after.ru_utime - usage.ru_utime, sys_seconds=after.ru_stime - usage.ru_stime)
    return row


def gnu_time_metrics(path):
    raw = path.read_text(encoding="utf-8")
    fields = raw.split()
    if len(fields) != 5:
        raise ValueError("missing/malformed GNU time metrics")
    values = [float(field) for field in fields]
    if any(not math.isfinite(value) or value < 0 for value in values):
        raise ValueError("invalid GNU time metrics")
    return dict(zip(("user_seconds", "sys_seconds", "peak_rss_kib", "voluntary_switches", "involuntary_switches"), values),
                gnu_time_raw=raw)


def elf_sizes(path):
    """ELF section/symbol bytes, not guessed objdump lengths or optimized objects."""
    data = path.read_bytes()
    if data[:4] != b"\x7fELF" or len(data) < 64 or data[4] not in (1, 2) or data[5] not in (1, 2):
        raise ValueError("unsupported/malformed ELF")
    order, wide = ("<" if data[5] == 1 else ">"), data[4] == 2
    header = struct.unpack_from(order + ("16sHHIQQQIHHHHHH" if wide else "16sHHIIIIIHHHHHH"), data)
    section_format = order + ("IIQQQQIIQQ" if wide else "IIIIIIIIII")
    size = struct.calcsize(section_format)
    offset, stride, count, names_index = header[6], header[11], header[12], header[13]
    if not count or names_index >= count or stride < size or offset + stride * count > len(data):
        raise ValueError("unsupported/malformed ELF section table (extended indices not inferred)")
    sections = [struct.unpack_from(section_format, data, offset + stride * i) for i in range(count)]
    def content(section):
        if section[4] + section[5] > len(data):
            raise ValueError("ELF section exceeds file")
        return data[section[4]:section[4] + section[5]]
    def name(table, index):
        if index >= len(table) or b"\0" not in table[index:]:
            raise ValueError("malformed ELF name")
        return table[index:table.index(b"\0", index)].decode("utf-8", errors="replace")
    names = content(sections[names_index])
    executable = []
    for section in sections:
        if section[2] & 4:
            if section[1] != 1:  # Executable bytes must be file-backed PROGBITS.
                raise ValueError("incompatible ELF executable section type")
            content(section)  # Validate even sections not otherwise decoded.
            executable.append({"name": name(names, section[0]), "bytes": section[5]})
    if not executable:
        raise ValueError("ELF has no executable sections")
    symbols = []
    symbol_format = order + ("IBBHQQ" if wide else "IIIBBH")
    symbol_size = struct.calcsize(symbol_format)
    for section in sections:
        if section[1] != 2:
            continue
        if section[6] >= count or section[9] < symbol_size or section[5] % section[9]:
            raise ValueError("malformed ELF symbol table")
        strings, table = content(sections[section[6]]), content(section)
        for position in range(0, len(table), section[9]):
            symbol = struct.unpack_from(symbol_format, table, position)
            index, info, defined, address, byte_count = ((symbol[0], symbol[1], symbol[3], symbol[4], symbol[5]) if wide else
                                                       (symbol[0], symbol[3], symbol[5], symbol[1], symbol[2]))
            symbol_name = name(strings, index)
            if info & 15 == 2 and defined and (symbol_name.startswith("__plenty_fn_") or symbol_name == "plenty_main"):
                symbols.append({"name": symbol_name, "address": address, "section": defined,
                                "st_size": byte_count, "usable_bytes": byte_count or None})
    if not symbols:
        raise ValueError("ELF source function symbols unavailable (no inferred sizes)")
    return {"sha256": digest(data), "file_bytes": len(data), "executable_sections": executable,
            "executable_section_bytes": sum(s["bytes"] for s in executable), "source_function_symbols": symbols}


def verify_approved_files(index_path, approved_sha256, paths):
    """The approval hash is supplied by the parent, never derived as approval."""
    if not index_path or not re.fullmatch(r"[0-9a-f]{64}", approved_sha256 or ""):
        raise ValueError("parent-approved evidence index path and SHA256 are required")
    data = Path(index_path).read_bytes()
    if digest(data) != approved_sha256:
        raise ValueError("approved evidence index pin mismatch")
    records = json.loads(data)
    if not isinstance(records, list):
        raise ValueError("approved evidence index must be a path-record list")
    identities = {}
    for record in records:
        path = record["path"]
        if not Path(path).is_absolute() or path in identities:
            raise ValueError("ambiguous approved evidence paths")
        identities[path] = record
    verified = {}
    for path in paths:
        path = str(Path(path).absolute())
        record = identities.get(path)
        if record is None or not re.fullmatch(r"[0-9a-f]{64}", record.get("sha256", "")):
            raise ValueError("file missing from approved evidence index: " + path)
        content = Path(path).read_bytes()
        if digest(content) != record["sha256"] or len(content) != record["size_bytes"]:
            raise ValueError("approved evidence file identity mismatch: " + path)
        verified[path] = record["sha256"]
    return verified


def allocation_evidence(directory, inputs, inventory_hash, perf_manifest, index_path=None, index_sha256=None):
    """Import marker outcomes bound to an independently pinned approved index."""
    root = Path(directory).resolve()
    file_names = ("validation.json", "input-inventory.json", "build-manifest.json", "result.json", "results.jsonl")
    paths = [root / name for name in file_names] + [root / mode / profile / name
            for mode in ("C", "I") for profile in ("debug", "release") for name in ("noalloc", "allocation-control")]
    approved = verify_approved_files(index_path, index_sha256, paths)
    metadata = json.loads((root / "validation.json").read_text())
    inventory = pinned_inventory(inputs, inventory_hash)
    if metadata["inventory_sha256"] != inventory_hash or digest((root / "input-inventory.json").read_bytes()) != inventory_hash:
        raise ValueError("allocation evidence inventory does not match measurement inputs")
    # Validate the original source files as well as the copied source/case inventory.
    pinned_inventory(metadata["inputs"], inventory_hash)
    checks = load_manifest(root / "build-manifest.json")
    for label in ("main", "candidate"):
        if checks["builds"][label]["source_sha"] != perf_manifest["builds"][label]["source_sha"]:
            raise ValueError("allocation evidence is from different source pins")
        if checks["builds"][label]["cargo_features"] != ["runtime-checks"]:
            raise ValueError("allocation evidence needs genuine runtime-checks builds")
        for key in ("target", "toolchain", "rustflags", "runtime_rustflags", "native_linker"):
            if role_value(checks["builds"][label][key], checks["builds"][label]) != role_value(perf_manifest["builds"][label][key], perf_manifest["builds"][label]):
                raise ValueError("allocation evidence toolchain/flags/target differs from perf provenance")
    before, after = (checks["builds"][label] for label in ("main", "candidate"))
    if any(role_value(before[key], before) != role_value(after[key], after) for key in MATCH_FIELDS):
        raise ValueError("allocation evidence build settings differ")
    result = json.loads((root / "result.json").read_text())
    if result.get("dry_run") or result.get("passed") is not True:
        raise ValueError("allocation evidence is not a successful actual validation")
    rows = [json.loads(line) for line in (root / "results.jsonl").read_text().splitlines()]
    artifacts = {}
    resolved_artifacts = set()
    for entry in inventory["entries"]:
        if entry["mode"] not in ("C", "I") or Path(entry["source"]).stem not in ("noalloc", "allocation-control"):
            continue
        label = "main" if entry["mode"] == "C" else "candidate"
        for profile in ("debug", "release"):
            group = [r for r in rows if r.get("source") == entry["source"] and r.get("mode") == entry["mode"] and r.get("profile") == profile]
            if len(group) != len(entry["cases"]) + 2:
                raise ValueError("allocation matrix cell incomplete or duplicated")
            for phase in ("check", "build"):
                commands = [r for r in group if r["phase"] == phase]
                if len(commands) != 1 or commands[0]["classification"] != "passed" or commands[0]["returncode"] != 0:
                    raise ValueError("missing/failed allocation check or build")
                if commands[0].get("timeout") or commands[0].get("spawn_error"):
                    raise ValueError("allocation check/build did not finish normally")
                if commands[0]["command"][0] != checks["builds"][label]["binary"]:
                    raise ValueError("allocation command does not use its frozen compiler")
            build = next(r for r in group if r["phase"] == "build")
            executable = Path(build["command"][build["command"].index("-o") + 1])
            wanted = root / entry["mode"] / profile / Path(entry["source"]).stem
            if executable.resolve() != wanted.resolve() or not executable.resolve().is_relative_to(root):
                raise ValueError("allocation artifact escapes or aliases its expected identity")
            source = str(Path(metadata["inputs"]).resolve() / entry["source"])
            binary = checks["builds"][label]["binary"]
            if next(r for r in group if r["phase"] == "check")["command"] != [binary, "--check", source] or build["command"] != (
                    [binary] + (["--release"] if profile == "release" else []) + ["--no-cache", "--no-strip", "--compile", source, "-o", str(executable)]):
                raise ValueError("allocation source/profile command mismatch")
            if executable.resolve() in resolved_artifacts:
                raise ValueError("aliased allocation executable artifacts")
            resolved_artifacts.add(executable.resolve())
            artifacts[str(executable)] = approved[str(executable)]
            for expected in entry["cases"]:
                actual = [r for r in group if r["phase"] == "runtime" and r.get("case") == expected["name"]]
                if len(actual) != 1 or actual[0]["command"] != [str(executable), *expected["args"]]:
                    raise ValueError("incomplete C/I allocation matrix")
                row = actual[0]
                if any(base64.b64decode(row[field + "_base64"], validate=True).decode("utf-8", errors="replace") != row[field]
                       for field in ("stdout", "stderr")):
                    raise ValueError("allocation raw streams disagree")
                if not classify(row, expected, entry["source"]).startswith("expected-"):
                    raise ValueError("allocation marker/control oracle failed")
    required = {(mode, name) for mode in ("C", "I") for name in ("noalloc", "allocation-control")}
    if {(e["mode"], Path(e["source"]).stem) for e in inventory["entries"] if e["requires_runtime_checks"]} != required:
        raise ValueError("allocation source/control coverage incomplete")
    verify_approved_files(index_path, index_sha256, paths)
    return {"status": "qualified", "scope": "zero/nonzero successful calling-thread allocations; not absolute totals",
            "source": str(root), "approved_index": {"path": str(Path(index_path).resolve()), "sha256": index_sha256},
            "files": {name: approved[str(root / name)] for name in file_names}, "executable_hashes": artifacts}


def measure(inputs, inventory_sha256, manifest_path, output, allow_perf=False, dry_run=False,
            phases=None, families=None, runtime_warmups=5, runtime_samples=30, compiler_warmups=3, compiler_samples=15,
            timeout=300, objdump="objdump", readelf="readelf", time_tool="/usr/bin/time", checks_evidence=None,
            checks_index=None, checks_index_sha256=None):
    if not allow_perf:
        raise ValueError("measurement requires --allow-perf")
    if resource is None:
        raise ValueError("measurement requires POSIX process-group and child resource accounting")
    if timeout <= 0:
        raise ValueError("timeout must be positive")
    inputs = Path(inputs).resolve()
    plan = measurement_plan(inputs, inventory_sha256, phases, families, runtime_warmups, runtime_samples,
                            compiler_warmups, compiler_samples)
    manifest_bytes = Path(manifest_path).read_bytes()
    manifest_hash = digest(manifest_bytes)
    manifest = load_manifest(manifest_path, measurement=True)
    if any(b["compiler_profile"] != "release" or b["cargo_features"] != [] for b in manifest["builds"].values()):
        raise ValueError("performance requires frozen release builds with no Cargo features")
    environment = manifest.get("execution_environment")
    if not isinstance(environment, dict) or not {"COMPILER_PATH", "PLENTY_LINKER_THREADS"} <= environment.keys() or any(
            key not in ("COMPILER_PATH", "PLENTY_LINKER_THREADS", "PATH") or
            (value is not None and not isinstance(value, str)) for key, value in environment.items()):
        raise ValueError("explicit execution_environment needs genuine linker settings (optional PATH)")
    if not checks_evidence and (checks_index is not None or checks_index_sha256 is not None):
        raise ValueError("approved index needs checks evidence")
    allocation = (allocation_evidence(checks_evidence, inputs, inventory_sha256, manifest, checks_index, checks_index_sha256)
                  if checks_evidence else {"status": "blocked", "reason": "no authentic matching checks-run evidence supplied"})
    root = fresh_directory(output)
    inventory = pinned_inventory(inputs, inventory_sha256)
    entries = {e["source"]: e for e in inventory["entries"] if e["source"] in plan["selected_sources"]}
    plan["build_manifest_sha256"] = manifest_hash
    write_json(root / "plan.json", plan)
    (root / "build-manifest.json").write_bytes(manifest_bytes)
    (root / "input-inventory.json").write_bytes((inputs / "inventory.json").read_bytes())
    env = dict(os.environ, PLENTY_CACHE="off", PLENTY_CACHE_DIR=str(root / "cache"), LC_ALL="C")
    for key in list(env):
        if key.startswith(("HOTPATH_", "PLENTY_PROFILE")):
            env.pop(key)
    for key, value in environment.items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    write_json(root / "environment.json", {key: env.get(key) for key in
               ("PATH", "COMPILER_PATH", "PLENTY_LINKER_THREADS", "PLENTY_CACHE", "PLENTY_CACHE_DIR", "LC_ALL")})
    write_json(root / "machine.json", {"system": platform.system(), "machine": platform.machine(), "affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
               "load": list(os.getloadavg()), "quiet_machine_approved": True,
               "policy": "read-only inventory; no CPU/affinity/ASLR/governor changes; OS caches not flushed"})
    ready, references, code_results, sample_rows = {}, {}, [], []
    artifact_hashes, artifact_errors, compiler_contracts = {}, [], {}

    def bind_artifact(path):
        key = str(path)
        sha256 = digest(Path(path).read_bytes())
        if key in artifact_hashes and artifact_hashes[key] != sha256:
            raise ValueError("bound artifact changed: " + key)
        artifact_hashes[key] = sha256
        return {"path": key, "sha256": sha256}

    def verify_artifacts(paths):
        for path in paths:
            if str(path) not in artifact_hashes or digest(Path(path).read_bytes()) != artifact_hashes[str(path)]:
                raise ValueError("bound artifact identity mismatch: " + str(path))

    def integrity_failure(command, error):
        message = str(error)
        artifact_errors.append(message)
        return dict(command=command, returncode=None, timeout=False, spawn_error=None,
                    stdout="", stderr="", stdout_base64="", stderr_base64="",
                    artifact_integrity_error=message)

    def qualified_command(command, child_env, limit):
        dependencies = [arg for arg in command if arg in artifact_hashes]
        try:
            verify_artifacts(dependencies)
        except (OSError, ValueError) as error:
            return integrity_failure(command, error)
        row = process_group(command, child_env, limit)
        try:
            verify_artifacts(dependencies)
            if "--compile" in command and row["returncode"] == 0 and not row["timeout"] and not row["spawn_error"]:
                row["artifact_identity"] = bind_artifact(Path(command[command.index("-o") + 1]))
            row["input_artifact_hashes"] = {path: artifact_hashes[path] for path in dependencies}
        except (OSError, ValueError) as error:
            artifact_errors.append(str(error))
            row["artifact_integrity_error"] = str(error)
        return row

    # These invocations have no approved diagnostic output on success. Confirm
    # that exact raw contract in untimed qualification before using it in jobs.
    compiler_stream_contract = {field: "" for field in ("stdout", "stderr", "stdout_base64", "stderr_base64")}

    def compiler_streams_match(row):
        return all(row.get(field) == expected for field, expected in compiler_stream_contract.items())

    with (root / "commands.jsonl").open("x", encoding="utf-8") as commands, (root / "samples.jsonl").open("x", encoding="utf-8") as samples:
        def record(row, sample=False):
            # A nonzero command exit can be an expected trap outcome. Keep
            # command status distinct from the later exact sample oracle.
            commands.write(json.dumps(dict(row, record_type="sample" if sample else "command"), sort_keys=True) + "\n")
            commands.flush()
            if sample:
                samples.write(json.dumps(row, sort_keys=True) + "\n")
                samples.flush()
                sample_rows.append(row)

        def run(command, context, timed=False, resource_file=None):
            if dry_run:
                row = dict(context, command=command, status="not-run")
            else:
                actual = ([time_tool, "-q", "-f", "%U %S %M %w %c", "-o", str(resource_file), *command]
                          if resource_file else command)
                dependencies = [arg for arg in command if arg in artifact_hashes]
                try:
                    verify_artifacts(dependencies)
                except (OSError, ValueError) as error:
                    row = dict(integrity_failure(actual, error), **context, status="blocked")
                    record(row)
                    return row
                row = dict(process_group(actual, env, timeout, timed=timed), **context)
                row["input_artifact_hashes"] = {path: artifact_hashes[path] for path in dependencies}
                try:
                    verify_artifacts(dependencies)
                except (OSError, ValueError) as error:
                    artifact_errors.append(str(error))
                    row["artifact_integrity_error"] = str(error)
                row["status"] = "passed" if row["returncode"] == 0 and not row["timeout"] and not row["spawn_error"] and not row.get("artifact_integrity_error") else "failed"
                if resource_file and row["status"] == "passed":
                    try:
                        row.update(gnu_time_metrics(resource_file), compiler_command=command,
                                   resource_scope="GNU time compiler and descendants; RSS KiB; wall includes wrapper")
                    except (OSError, ValueError) as error:
                        row.update(status="failed", metric_error=str(error))
            record(row)
            return row

        if entries:
            # Reuse the semantic qualifier, including both generated profiles,
            # every exact oracle and native retention gates, without timing it.
            validate(inputs, manifest_path, root / "qualification", dry_run, timeout,
                     sorted({Path(e["source"]).stem for e in entries.values()}), None, objdump,
                     runner=qualified_command, child_environment=env)
            qualifications = [json.loads(line) for line in (root / "qualification/results.jsonl").read_text().splitlines()]
            for entry in entries.values():
                for profile in ("debug", "release"):
                    group = [r for r in qualifications if r["source"] == entry["source"] and r["profile"] == profile and r["mode"] == entry["mode"]]
                    ready[entry["source"], profile] = dry_run or (
                        len([r for r in group if r["phase"] == "runtime"]) == len(entry["cases"]) and
                        all(r["classification"] in ("passed", "retained-call") or r["classification"].startswith("expected-") for r in group))
                    for phase, action in (("check", "check"), ("build", "aot-" + profile)):
                        observations = [r for r in group if r["phase"] == phase]
                        contract_ready = dry_run or (len(observations) == 1 and observations[0]["classification"] == "passed" and compiler_streams_match(observations[0]))
                        compiler_contracts[entry["source"], action] = compiler_contracts.get((entry["source"], action), True) and contract_ready
                        record(dict(phase="compiler-stream-contract", source=entry["source"], mode=entry["mode"],
                                    profile=profile, action=action, expected=compiler_stream_contract,
                                    status="not-run" if dry_run else "passed" if contract_ready else "blocked"))
        for entry in entries.values():
            source = str(inputs / entry["source"])
            mode, family = entry["mode"], Path(source).stem
            compiler = manifest["builds"]["main" if mode in ("T", "C") else "candidate"]["binary"]
            if set(plan["phases"]) & {"compiler", "code"}:
                artifact = root / "reference" / mode / (family + ".o")
                artifact.parent.mkdir(parents=True, exist_ok=True)
                context = dict(phase="reference-object", source=entry["source"], mode=mode)
                if not ready.get((entry["source"], "debug")):
                    record(dict(context, status="blocked", reason="qualification failed"))
                else:
                    row = run([compiler, "--emit-object", source, "-o", str(artifact)], context)
                    if dry_run or row["status"] == "passed" and artifact.is_file() and compiler_streams_match(row):
                        references[entry["source"]] = artifact
                        compiler_contracts[entry["source"], "object-debug"] = True
                        if not dry_run:
                            try:
                                identity = bind_artifact(artifact)
                                record(dict(context, phase="reference-object-identity", artifact_identity=identity, status="passed"))
                            except (OSError, ValueError) as error:
                                artifact_errors.append(str(error))
                                ready[entry["source"], "debug"] = False
                                record(dict(context, phase="reference-object-identity", status="failed", artifact_integrity_error=str(error)))
                    else:
                        ready[entry["source"], "debug"] = False
                        record(dict(context, phase="compiler-stream-contract", action="object-debug",
                                    expected=compiler_stream_contract, status="blocked"))
            if "code" in plan["phases"]:
                context = dict(phase="code", source=entry["source"], mode=mode)
                if not all(ready.get((entry["source"], p)) for p in ("debug", "release")) or entry["source"] not in references:
                    code_results.append(dict(context, status="blocked", reason="qualification/object emission failed"))
                    continue
                executable = root / "qualification" / mode / "release" / family
                destination = root / "code" / mode / family
                destination.mkdir(parents=True)
                primary = ("work" + str(int(family.removeprefix("corpus-")) - 1) if family.startswith("corpus-") else
                           {"tight": "kernel", "calls": "calls", "text": "consume", "recovered-errors": "main"}[family])
                dump_command = [compiler, "--release", "--dump", "all", "--dump-std", source, "-o", str(destination / "dump")]
                if family.startswith("corpus-"):
                    dump_command[1:1] = ["--dump-filter", primary]  # Explicit representative workN, not a full corpus IR audit.
                results = [run([readelf, "-SW", str(executable)], dict(context, extraction="ELF-sections")),
                           run([readelf, "-Ws", str(executable)], dict(context, extraction="ELF-symbols")),
                           run([objdump, "-drw", "--no-show-raw-insn", "--disassemble=__plenty_fn_" + primary, str(executable)], dict(context, extraction="native-primary")),
                           run(dump_command, dict(context, extraction="compiler-dump"))]
                item = dict(context, status="not-run" if dry_run else "passed")
                if not dry_run:
                    try:
                        if any(r["status"] != "passed" for r in results):
                            raise ValueError("native/IR extraction command failed")
                        try:
                            verify_artifacts([executable, references[entry["source"]]])
                        except (OSError, ValueError) as error:
                            artifact_errors.append(str(error))
                            raise
                        item.update(optimized_linked_ELF=elf_sizes(executable), debug_application_object=elf_sizes(references[entry["source"]]))
                        if (item["optimized_linked_ELF"]["sha256"] != artifact_hashes[str(executable)] or
                                item["debug_application_object"]["sha256"] != artifact_hashes[str(references[entry["source"]])]):
                            artifact_errors.append("code extraction artifact identity mismatch")
                            raise ValueError(artifact_errors[-1])
                        symbol = "__plenty_fn_" + primary
                        if symbol not in {s["name"] for s in item["optimized_linked_ELF"]["source_function_symbols"]} or ("<" + symbol + ">:") not in results[2]["stdout"]:
                            raise ValueError("primary native function extraction missing")
                        files = list((destination / "dump").glob("*.txt"))
                        if {p.name for p in files} != {"index.txt", "ops.txt", "clif.txt", "clif-opt.txt", "asm.txt"} or any(not p.stat().st_size for p in files):
                            raise ValueError("incomplete compiler dump")
                        if symbol not in (destination / "dump/index.txt").read_text():
                            raise ValueError("primary function missing from compiler dump index")
                        item["dump_hashes"] = {p.name: digest(p.read_bytes()) for p in files}
                    except (ValueError, OSError, struct.error) as error:
                        item.update(status="failed", reason=str(error))
                        ready[entry["source"], "release"] = False
                code_results.append(item)
                write_json(destination / "sizes.json", item)
        blocked_groups = set()
        for job in plan["jobs"]:
            entry = entries[job["source"]]
            source = str(inputs / entry["source"])
            mode, family = job["mode"], job["family"]
            key = (job["stage"], job["source"], job["case"])
            required = ("release",) if job["stage"] == "runtime" else ("debug", "release")
            dependencies = [root / "qualification" / mode / profile / family for profile in required]
            if job["stage"] == "compiler" and entry["source"] in references:
                dependencies.append(references[entry["source"]])
            if not dry_run:
                job = dict(job, qualified_artifact_hashes={str(path): artifact_hashes.get(str(path)) for path in dependencies})
            if key in blocked_groups or not all(ready.get((entry["source"], p)) for p in required):
                record(dict(job, status="blocked", reason="prior sample or qualification/extraction failure"), sample=True)
                continue
            directory = root / "work" / job["id"]
            directory.mkdir(parents=True)
            prepared = root / "qualification" / mode / "release" / family
            if job["stage"] == "compiler":
                if not compiler_contracts.get((entry["source"], job["action"])):
                    blocked_groups.add(key)
                    record(dict(job, status="blocked", reason="untimed compiler stream contract unavailable"), sample=True)
                    continue
            if not dry_run:
                try:
                    verify_artifacts(dependencies)
                except (OSError, ValueError) as error:
                    artifact_errors.append(str(error))
                    blocked_groups.add(key)
                    record(dict(job, status="blocked", artifact_integrity_error=str(error)), sample=True)
                    continue
            if job["stage"] == "runtime":
                expected = next(c for c in entry["cases"] if c["name"] == job["case"])
                row = run([str(prepared), *expected["args"]], job, timed=True)
                if not dry_run:
                    row["classification"] = classify(row, expected, source)
                    if row["status"] != "blocked":
                        row["status"] = "passed" if row["classification"].startswith("expected-") else "failed"
            else:
                compiler = manifest["builds"]["main" if mode in ("T", "C") else "candidate"]["binary"]
                artifact = directory / ("program.o" if job["action"] == "object-debug" else "program")
                action = job["action"]
                command = ([compiler, "--check", source] if action == "check" else
                           [compiler, "--emit-object", source, "-o", str(artifact)] if action == "object-debug" else
                           [compiler] + (["--release"] if action == "aot-release" else []) + ["--no-cache", "--no-strip", "--compile", source, "-o", str(artifact)])
                row = run(command, job, timed=True, resource_file=directory / "resources.txt")
                if not dry_run:
                    row["compiler_stream_contract"] = compiler_stream_contract
                    if row["status"] == "passed" and not compiler_streams_match(row):
                        row.update(status="failed", compiler_stream_error="unexpected raw compiler stdout/stderr")
                    try:
                        verify_artifacts(dependencies)
                    except (OSError, ValueError) as error:
                        artifact_errors.append(str(error))
                        row.update(status="failed", artifact_integrity_error=str(error))
                    if action != "check" and (row["status"] == "passed" or artifact.is_file()):
                        try:
                            row["artifact_identity"] = bind_artifact(artifact)
                        except (OSError, ValueError) as error:
                            artifact_errors.append(str(error))
                            row.update(status="failed", artifact_integrity_error=str(error))
                if not dry_run and row["status"] == "passed":
                    try:
                        if action == "object-debug":
                            if row["artifact_identity"]["sha256"] != artifact_hashes[str(references[entry["source"]])]:
                                raise ValueError("sample object differs from untimed reference object")
                        verifier = artifact if action.startswith("aot-") else root / "qualification" / mode / "debug" / family
                        row["oracle_scope"] = "new AOT executable" if action.startswith("aot-") else "fixed-source preflight executable; object samples additionally require byte identity"
                        for expected in entry["cases"]:
                            proof = run([str(verifier), *expected["args"]], dict(phase="sample-oracle", job_id=job["id"], case=expected["name"]))
                            if not classify(proof, expected, source).startswith("expected-"):
                                raise ValueError("compiler sample executable oracle failed")
                        row["artifact_bytes"] = artifact.stat().st_size if action != "check" else None
                    except (OSError, ValueError) as error:
                        row.update(status="failed", oracle_error=str(error))
            if not dry_run and row["status"] != "passed":
                blocked_groups.add(key)
            record(row, sample=True)
    integrity_error = None
    try:
        pinned_inventory(inputs, inventory_sha256)
        if digest(Path(manifest_path).read_bytes()) != manifest_hash:
            raise ValueError("build manifest changed during measurement")
        load_manifest(manifest_path, measurement=True)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        artifact_errors.append(str(error))
    # Recheck independent identities even when a different integrity guard failed.
    try:
        verify_artifacts(artifact_hashes)
    except (ValueError, OSError) as error:
        artifact_errors.append(str(error))
    if allocation["status"] == "qualified":
        try:
            verify_approved_files(allocation["approved_index"]["path"], allocation["approved_index"]["sha256"],
                                  [Path(allocation["source"]) / name for name in allocation["files"]] + list(allocation["executable_hashes"]))
        except (ValueError, OSError) as error:
            artifact_errors.append(str(error))
    if artifact_errors:
        integrity_error = "; ".join(artifact_errors)
    write_json(root / "artifact-identities.json", artifact_hashes)
    phase_status = {}
    for phase in plan["phases"]:
        if dry_run:
            phase_status[phase] = "not-run"
        elif integrity_error:
            phase_status[phase] = "invalid-integrity"
        elif phase == "allocation":
            phase_status[phase] = "passed" if allocation["status"] == "qualified" else "blocked"
        elif phase == "code":
            phase_status[phase] = "passed" if code_results and all(r["status"] == "passed" for r in code_results) else "failed"
        else:
            phase_status[phase] = "passed" if all(r["status"] == "passed" for r in sample_rows if r["stage"] == phase) else "failed"
    supported_passed = all(status == "passed" for status in phase_status.values()) and not integrity_error
    summary = {"dry_run": dry_run, "supported_phases_passed": None if dry_run else supported_passed,
               "measurements_executed": not dry_run and any(r["status"] in ("passed", "failed") for r in sample_rows),
               "phase_status": phase_status, "packet_complete": False, "acceptance_complete": False,
               "integrity_error": integrity_error, "blocked_metrics": NOT_IMPLEMENTED,
               "allocation": allocation, "code": code_results, "scope": plan["timing_scope"],
               "protocol": plan["protocol"], "statistics": sample_summary(sample_rows) if not dry_run and not integrity_error else None}
    write_json(root / "result.json", summary)
    return supported_passed if not dry_run else True


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
    check.add_argument("--objdump", default="objdump", help="native disassembler for mandatory optimized call-retention gates")
    for name in ("plan", "measure"):
        command = phases.add_parser(name, help="native-free job plan" if name == "plan" else "explicitly authorized whole-process measurement")
        command.add_argument("--inputs", required=True, type=Path)
        command.add_argument("--inventory-sha256", required=True)
        command.add_argument("--output", required=True, type=Path)
        command.add_argument("--phase", dest="measurement_phases", choices=MEASURE_PHASES, action="append")
        command.add_argument("--family", action="append", help="runtime families only; compiler/code always retain full 1000/4000 corpus")
        command.add_argument("--runtime-warmups", type=int, default=5)
        command.add_argument("--runtime-samples", type=int, default=30)
        command.add_argument("--compiler-warmups", type=int, default=3)
        command.add_argument("--compiler-samples", type=int, default=15)
        if name == "measure":
            command.add_argument("--allow-perf", action="store_true")
            command.add_argument("--build-manifest", required=True, type=Path)
            command.add_argument("--checks-evidence", type=Path)
            command.add_argument("--checks-index", type=Path, help="independently approved artifact identity index")
            command.add_argument("--checks-index-sha256", help="parent-supplied approval pin, not a self-computed approval")
            command.add_argument("--dry-run", action="store_true")
            command.add_argument("--timeout", type=int, default=300)
            command.add_argument("--objdump", default="objdump")
            command.add_argument("--readelf", default="readelf")
            command.add_argument("--time-tool", default="/usr/bin/time")
    args = parser.parse_args(argv)
    try:
        if args.phase == "generate":
            generate(args.output, allocations=args.allocation_probes)
        elif args.phase == "validate":
            return 0 if validate(args.inputs, args.build_manifest, args.output, args.dry_run,
                                 args.timeout, args.family, args.mode, args.objdump) else 1
        else:
            options = dict(phases=args.measurement_phases, families=args.family,
                           runtime_warmups=args.runtime_warmups, runtime_samples=args.runtime_samples,
                           compiler_warmups=args.compiler_warmups, compiler_samples=args.compiler_samples)
            if args.phase == "plan":
                plan = measurement_plan(args.inputs, args.inventory_sha256, **options)
                plan["execution"] = "not-run; no native commands or clocks"
                write_json(fresh_directory(args.output) / "plan.json", plan)
            else:
                return 0 if measure(args.inputs, args.inventory_sha256, args.build_manifest, args.output,
                                    allow_perf=args.allow_perf, dry_run=args.dry_run, timeout=args.timeout,
                                    objdump=args.objdump, readelf=args.readelf, time_tool=args.time_tool,
                                    checks_evidence=args.checks_evidence, checks_index=args.checks_index,
                                    checks_index_sha256=args.checks_index_sha256, **options) else 1
    except (ValueError, OSError, KeyError, TypeError, struct.error, subprocess.CalledProcessError) as error:
        parser.exit(2, f"error: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
