#!/usr/bin/env python3
"""Reproducible cold serialization compile/object-size and throughput probes.

Run with a compiler built from the measured checkout. Sources, objects, binaries,
timings and compiler identity stay in the selected output directory. No caches
are read or populated. `manual` uses the same ordinary hooks as derivation.
"""

import argparse
import hashlib
import json
from pathlib import Path
import platform
import statistics
import subprocess
import time


def record(name, fields, mode, first_type="i64"):
    def field_type(index):
        return first_type if index == 0 else "i64"
    lines = (["@serial"] if mode == "derived" else []) + [f"class {name}:"]
    lines += [f"    f{i}: {field_type(i)}" for i in range(fields)]
    if mode not in ("manual", "legacy"):
        return "\n".join(lines)
    lines += [
        "    def __encode__[E, S: std.serial.Encoder[E]](self, out: &mut S) -> Result[(), std.serial.SerialError[E]]:",
        "        std.serial.wrap(out.start_object())?",
    ]
    for i in range(fields):
        lines += [
            f'        std.serial.at(std.serial.wrap(out.field("f{i}")), std.serial.Step.Field("f{i}"))?',
            f'        std.serial.at(std.serial.encode[{field_type(i)}, E, S](&self.f{i}, out), std.serial.Step.Field("f{i}"))?',
        ]
    lines += [
        "        std.serial.wrap(out.end_object())",
        f"    def __decode__[E, D: std.serial.Decoder[E]](source: &mut D) -> Result[{name}, std.serial.SerialError[E]]:",
        "        std.serial.wrap(source.start_object())?",
    ]
    if mode == "manual":
        tuple_type = "(" + ", ".join(f"Option[{field_type(i)}]" for i in range(fields)) + ",)"
        tuple_value = "(" + ", ".join("Nothing" for _ in range(fields)) + ",)"
        lines += [f"        mut slots: {tuple_type} = {tuple_value}",
                  "        while True:",
                  "            match std.serial.wrap(source.next_key())?:",
                  "                case Some(name):",
                  "                    match name:"]
        for i in range(fields):
            lines += [f'                        case "f{i}":',
                      f'                            std.serial.read_field[{field_type(i)}, E, D](&mut slots[{i}], source, "f{i}")?']
        lines += ["                        case _:",
                  "                            std.serial.at(std.serial.wrap(source.unknown_key(&name)), std.serial.Step.Field(name))?",
                  "                case Nothing:",
                  "                    break"]
        args = ", ".join(f'f{i}=std.serial.require_field[{field_type(i)}, E, D](&mut slots[{i}], source, "f{i}")?' for i in range(fields))
        lines += [f"        construct_{name}[E, D](&mut slots, source)", "",
                  f"def construct_{name}[E, D: std.serial.Decoder[E]](slots: &mut {tuple_type}, source: &D) -> Result[{name}, std.serial.SerialError[E]]:",
                  f"    Ok({name}({args}))"]
        return "\n".join(lines)
    lines += [f"        mut slot{i}: Option[{field_type(i)}] = Nothing" for i in range(fields)]
    lines += [
        "        while True:",
        "            match std.serial.wrap(source.next_key())?:",
        "                case Some(name):",
        "                    match name:",
    ]
    for i in range(fields):
        lines += [
            f'                        case "f{i}":',
            f"                            if slot{i}.is_some():",
            "                                return Err(std.serial.error(source.reject(std.serial.Fault.DuplicateField(name))))",
            f'                            slot{i} = Some(std.serial.at(std.serial.decode[{field_type(i)}, E, D](source), std.serial.Step.Field("f{i}"))?)',
        ]
    lines += [
        "                        case _:",
        "                            std.serial.at(std.serial.wrap(source.unknown_key(&name)), std.serial.Step.Field(name))?",
        "                case Nothing:",
        "                    break",
    ]
    for i in range(fields):
        lines += [
            f"        value{i} = match slot{i}:",
            "            case Some(value):",
            "                value",
            "            case Nothing:",
            f'                return Err(std.serial.error(source.reject(std.serial.Fault.MissingField("f{i}"))))',
        ]
    lines += [f"        Ok({name}({', '.join(f'value{i}' for i in range(fields))}))"]
    return "\n".join(lines)


def classes(count, fields, mode, fmt):
    source = [f"import std.{fmt}", "import std.serial"]
    source += [record(f"C{i}", fields, mode) for i in range(count)]
    result = f"Result[i64, AllocError | IoError | std.serial.SerialError[std.{fmt}.{fmt.title()}Error]]"
    for i in range(count):
        lines = [f"def probe{i}() -> {result}:",
                 f"    original = C{i}({', '.join(str(j) for j in range(fields))})"]
        if mode == "plain":
            lines += ["    Ok(original.f0)"]
        else:
            lines += [f"    bounds = std.{fmt}.limits()",
                      f"    options = std.{fmt}.codec_options()",
                      f"    text = std.{fmt}.dump(&original, &{'bounds' if fmt == 'json' else 'options'})?",
                      f"    value = std.{fmt}.load[C{i}](&text, &options)?",
                      "    Ok(value.f0)"]
        source.append("\n".join(lines))
    lines = [f"def main() -> Result[(), AllocError | IoError | OverflowError[i64] | std.serial.SerialError[std.{fmt}.{fmt.title()}Error]]:",
             "    mut total = 0"]
    for i in range(count):
        lines += [f"    total += probe{i}()?"]
    lines += ["    print(total)?", "    Ok(())"]
    source.append("\n".join(lines))
    return "\n\n".join(source) + "\n"



def elapsed(command, log):
    start = time.perf_counter()
    process = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    log.write_text(process.stdout + process.stderr)
    if process.returncode:
        raise RuntimeError(f"command failed ({process.returncode}); see {log}")
    return time.perf_counter() - start


def throughput(mode):
    payload = record("Payload", 1, mode, first_type="str")
    source = ["import std.json", "import std.serial", "import std.bytes", "import std.time", payload]
    source += [record(f"Fields{n}", n, mode) for n in (4, 16, 64)]
    for n in (4, 16, 64):
        dispatch = [f"def dispatch{n}(input: &mut std.json.CodecDecoder) -> Result[i64, std.json.JsonError | OverflowError[i64]]:", "    input.start_object()?", "    mut total = 0", "    while True:", "        match input.next_key()?:", "            case Some(name):", "                match name:"]
        for i in range(n):
            dispatch += [f'                    case "f{i}":', "                        total += input.integer()?"]
        dispatch += ["                    case _:", "                        input.unknown_key(&name)?", "            case Nothing:", "                break", "    input.finish()?", "    Ok(total)"]
        source.append("\n".join(dispatch))
    lines = [
        "def main() -> Result[(), AllocError | IoError | OverflowError[i64] | std.bytes.BytesError | std.json.JsonError | std.serial.SerialError[std.json.JsonError]]:",
        "    bounds = std.json.limits()",
        "    options = std.json.codec_options()",
        "    tree_bounds = std.json.tree_limits()",
        "    mut storage = std.bytes.empty()?",
        "    storage.resize(8388608, 97u8)?",
        "    payload = Payload(storage.to_text()?)",
        "    text = std.json.dump(&payload, &bounds)?",
        "    tree = std.json.to_value(&payload, &tree_bounds)?",
        "    mut checksum = 0",
    ]
    kernels = {
        "typed_decode": ["value = std.json.load[Payload](&text, &options)?", "checksum += value.f0.byte_len()"],
        "tree_parse": ["value = std.json.parse(&text, &bounds)?", "drop(value)", "checksum += text.len()"],
        "event_decode": ["mut input = std.json.codec_decoder(&text, &options)?", "input.start_object()?", "drop(input.next_key()?)", "value = input.text()?", "drop(input.next_key()?)", "input.finish()?", "checksum += value.byte_len()"],
        "typed_encode": ["value = std.json.dump(&payload, &bounds)?", "checksum += value.len()"],
        "tree_encode": ["value = std.json.encode(&tree, &bounds)?", "checksum += value.len()"],
        "event_encode": ["mut output = std.json.encoder(&bounds)?", "output.start_object()?", 'output.key("f0")?', "output.text(&payload.f0)?", "output.end_object()?", "value = output.take()?", "checksum += value.len()"],
    }
    for index, (name, operations) in enumerate(kernels.items()):
        lines += [f"    start{index} = std.time.monotonic_now()", "    for _ in range(5):"]
        lines += ["        " + line for line in operations]
        lines += [f'    print("{name}", std.time.monotonic_now().saturating_duration_since(&start{index}).as_nanoseconds())?']
    for n in (4, 16, 64):
        lines += [f"    fields{n} = Fields{n}({', '.join(str(i) for i in range(n))})", f"    text{n} = std.json.dump(&fields{n}, &bounds)?", f"    begin{n} = std.time.monotonic_now()", "    for _ in range(2000):", f"        mut input = std.json.codec_decoder(&text{n}, &options)?", f"        checksum += dispatch{n}(&mut input)?", f'    print("fields_{n}", std.time.monotonic_now().saturating_duration_since(&begin{n}).as_nanoseconds())?']
    lines += ["    typed64 = std.json.load[Fields64](&text64, &options)?", "    checksum += typed64.f63"]
    lines += ['    print("checksum", checksum)?' , "    Ok(())"]
    return "\n\n".join(source) + "\n\n" + "\n".join(lines) + "\n"


def mixed_fixture():
    """Match the documented JSON cost probe's bytes, objects and event count.

    The historical row contents were not retained. Nine fields produce twenty
    events per object; distribute harmless ASCII padding to recover its exact
    byte size without changing that structural workload.
    """
    count, size = 34393, 8388872
    blank = {"f0": "", **{f"f{i}": i for i in range(1, 9)}}
    base = json.dumps(blank, separators=(",", ":")).encode()
    padding, extra = divmod(size - count * (len(base) + 1) - 1, count)
    rows = []
    for i in range(count):
        row = dict(blank)
        row["f0"] = "x" * (padding + (i < extra))
        rows.append(json.dumps(row, separators=(",", ":")).encode())
    value = b"[" + b",".join(rows) + b"]"
    assert len(value) == size
    return value


def mixed_throughput(mode):
    row = record("Row", 9, mode, first_type="str")
    source = ["import std.json", "import std.serial", "import std.time", 'const fixture: bytes = include_bytes("mixed.json")', row]
    lines = [
        "def main() -> Result[(), AllocError | IoError | OverflowError[i64] | std.json.JsonError | std.serial.SerialError[std.json.JsonError]]:",
        "    bounds = std.json.limits()",
        "    options = std.json.codec_options()",
        "    typed = std.json.load[list[Row]](&fixture, &options)?",
        "    tree = std.json.parse(&fixture, &bounds)?",
        "    mut checksum = 0",
    ]
    kernels = {
        "typed_decode": ["value = std.json.load[list[Row]](&fixture, &options)?", "checksum += len(value)"],
        "tree_parse": ["value = std.json.parse(&fixture, &bounds)?", "drop(value)", "checksum += 34393"],
        "event_scan": ["mut input = std.json.decoder(&bounds)?", "input.feed(&fixture)?", "input.finish()", "while True:", "    match input.next()?:", "        case std.json.Event.End:", "            break", "        case _:", "            checksum += 1"],
        "typed_encode": ["value = std.json.dump(&typed, &bounds)?", "checksum += value.len()"],
        "tree_encode": ["value = std.json.encode(&tree, &bounds)?", "checksum += value.len()"],
    }
    for i, (name, operations) in enumerate(kernels.items()):
        lines += [f"    start{i} = std.time.monotonic_now()", "    for _ in range(1):"]
        lines += ["        " + line for line in operations]
        lines += [f'    print("{name}", std.time.monotonic_now().saturating_duration_since(&start{i}).as_nanoseconds())?']
    lines += ['    print("checksum", checksum)?', "    Ok(())"]
    return "\n\n".join(source) + "\n\n" + "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--modes", nargs="+", default=["plain", "manual", "derived"])
    parser.add_argument("--formats", nargs="+", default=["json", "toml"])
    parser.add_argument("--count", type=int, default=200)
    parser.add_argument("--fields", type=int, default=10)
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--throughput", action="store_true", help="also run JSON 8 MiB and 4/16/64-field probes")
    parser.add_argument("--mixed", action="store_true", help="also reproduce the documented 8,388,872-byte, 687,862-event JSON shape")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    report = {"platform": platform.platform(), "compiler": str(args.compiler.resolve()), "count": args.count, "fields": args.fields, "results": []}
    report["version"] = subprocess.check_output([args.compiler, "--version"], text=True).strip()
    report["source_revision"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=Path(__file__).resolve().parent.parent, text=True).strip()
    report["compiler_sha256"] = hashlib.file_digest(args.compiler.open("rb"), "sha256").hexdigest()
    for fmt in args.formats:
        for mode in args.modes:
            name = f"{fmt}-{mode}-{args.count}x{args.fields}"
            source = args.out / f"{name}.plenty"
            source.write_text(classes(args.count, args.fields, mode, fmt))
            obj = source.with_suffix(".o")
            seconds = []
            for run in range(args.repeat):
                seconds.append(elapsed([str(args.compiler), "--no-cache", "--emit-object", str(source), "-o", str(obj)], args.out / f"{name}-{run}.log"))
            result = {"format": fmt, "mode": mode, "compile_seconds": seconds, "median_seconds": statistics.median(seconds), "object_bytes": obj.stat().st_size}
            report["results"].append(result)
            (args.out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps(result), flush=True)
    if args.mixed:
        fixture = mixed_fixture()
        (args.out / "mixed.json").write_bytes(fixture)
        for mode in args.modes:
            if mode not in ("manual", "derived"):
                continue
            name = f"json-{mode}-mixed"
            source = args.out / f"{name}.plenty"
            source.write_text(mixed_throughput(mode))
            binary = source.with_suffix("")
            elapsed([str(args.compiler), "--no-cache", "--compile", str(source), "-o", str(binary)], args.out / f"{name}-compile.log")
            runs = []
            for run in range(args.repeat):
                log = args.out / f"{name}-{run}.log"
                elapsed([str(binary)], log)
                runs.append(dict((key, int(value)) for key, value in (line.split() for line in log.read_text().splitlines())))
            result = {"mode": mode, "document_bytes": len(fixture), "objects": 34393, "events": 687862, "fixture_sha256": hashlib.sha256(fixture).hexdigest(), "mixed_nanoseconds": runs}
            report["results"].append(result)
            (args.out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps(result), flush=True)
    if args.throughput:
        for mode in args.modes:
            if mode == "plain":
                continue
            name = f"json-{mode}-throughput"
            source = args.out / f"{name}.plenty"
            source.write_text(throughput(mode))
            binary = source.with_suffix("")
            elapsed([str(args.compiler), "--no-cache", "--compile", str(source), "-o", str(binary)], args.out / f"{name}-compile.log")
            runs = []
            for run in range(args.repeat):
                log = args.out / f"{name}-{run}.log"
                elapsed([str(binary)], log)
                runs.append(dict((key, int(value)) for key, value in (line.split() for line in log.read_text().splitlines())))
            result = {"mode": mode, "document_payload_bytes": 8388608, "document_iterations": 5, "field_iterations": 2000, "throughput_nanoseconds": runs}
            report["results"].append(result)
            (args.out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()
