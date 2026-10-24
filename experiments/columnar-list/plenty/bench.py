#!/usr/bin/env python3
"""Measure generated and optional native Plenty columns against list[Particle]."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess

from generate import generate

HERE = Path(__file__).resolve().parent
FIELDS = ("x", "y", "z", "vx", "vy", "vz", "mass", "tag")


def program(rows, repeats, samples, native=False):
    kernels = []
    full = "row.x * row.y + row.z * row.vx + row.vy * row.vz + row.mass * row.tag"
    layouts = [("aos", "list[Particle]"), ("soa", "ClistParticle")]
    if native:
        layouts.append(("native", "clist[Particle]"))
    for layout, ty in layouts:
        for kind in ["one", "two", "stream", "whole", "random"]:
            lines = [f"def {layout}_{kind}(items: &{ty}) -> i64:"]
            if layout == "native" and kind in {"one", "two"}:
                if kind == "one":
                    lines += ["    items.column(Particle.x).fold(0, add_value)"]
                else:
                    lines += ["    items.column(Particle.x).zip(items.column(Particle.y)).fold(0, filtered_value)"]
            elif layout == "native" and kind == "stream":
                expression = full.replace("row.", "items[i].")
                lines += ["    mut total = 0", f"    for i in range({rows}):",
                          f"        total += {expression}", "    total"]
            elif layout == "soa" and kind in {"one", "two"}:
                lines += ["    xs = items.column_x()"]
                if kind == "one":
                    lines += ["    iter(xs).fold(0, add)"]
                else:
                    lines += ["    ys = items.column_y()", "    iter(xs).zip(ys).fold(0, filtered)"]
            elif layout == "soa" and kind == "stream":
                lines += [f"    {field}s = items.column_{field}()" for field in FIELDS]
                lines += ["    mut total = 0", f"    for i in range({rows}):",
                          "        total += xs[i] * ys[i] + zs[i] * vxs[i] + vys[i] * vzs[i] + masss[i] * tags[i]",
                          "    total"]
            else:
                lines += ["    mut total = 0"]
                if kind == "random":
                    lines += [f"    for i in range({rows}):", f"        index = (i * 65537) % {rows}"]
                elif layout in {"soa", "native"}:
                    lines += [f"    for index in range({rows}):"]
                else:
                    lines += ["    for row in items:"]
                if layout in {"soa", "native"}:
                    lines += ["        row = items.copy_row(index).unwrap()"]
                elif kind == "random":
                    lines += ["        row = &items[index]"]
                expr = {"one": "row.x", "two": "row.y if row.x % 2 == 0 else 0"}.get(kind, full)
                lines += [f"        total += {expr}", "    total"]
            kernels += ["\n".join(lines)]
            kernels += [f"""def measure_{layout}_{kind}(items: &{ty}, sample: i64) -> Result[(), IoError]:
    begin = monotonic_now()
    mut checksum = 0
    for repeat in range({repeats}):
        checksum += {layout}_{kind}(items)
    end = monotonic_now()
    print("{layout}_{kind}", sample, end.saturating_duration_since(&begin).as_nanoseconds(), checksum)?
    Ok(())"""]
    source = """from columns import Particle, ClistParticle, AppendParticleError
from std.time import monotonic_now

def add(total: i64, value: &i64) -> i64:
    total + *value

def filtered(total: i64, x: &i64, y: &i64) -> i64:
    total + *y if *x % 2 == 0 else total

def add_value(total: i64, value: i64) -> i64:
    total + value

def filtered_value(total: i64, x: i64, y: i64) -> i64:
    total + y if x % 2 == 0 else total

""" + "\n\n".join(kernels)
    extra_error = " | AppendError[Particle]" if native else ""
    source += f"""

def main() -> Result[(), AllocError | IoError | AppendParticleError{extra_error}]:
    start_aos = monotonic_now()
    mut aos = list[Particle].with_capacity({rows})?
    for i in range({rows}):
        n = i % 1024
        aos.append(Particle(n, n + 1, n + 2, n + 3, n + 4, n + 5, n + 6, n + 7))?
    end_aos = monotonic_now()
    print("build_aos", end_aos.saturating_duration_since(&start_aos).as_nanoseconds(), len(aos))?
    start_soa = monotonic_now()
    mut soa = ClistParticle()?
    soa.reserve({rows})?
    for i in range({rows}):
        n = i % 1024
        soa.append(Particle(n, n + 1, n + 2, n + 3, n + 4, n + 5, n + 6, n + 7))?
    end_soa = monotonic_now()
    print("build_soa", end_soa.saturating_duration_since(&start_soa).as_nanoseconds(), soa.len())?
"""
    if native:
        source += f"""    start_native = monotonic_now()
    mut native = clist[Particle].with_capacity({rows})?
    for i in range({rows}):
        n = i % 1024
        native.append(Particle(n, n + 1, n + 2, n + 3, n + 4, n + 5, n + 6, n + 7))?
    end_native = monotonic_now()
    print("build_native", end_native.saturating_duration_since(&start_native).as_nanoseconds(), len(native))?
"""
    # All owners remain allocated. Alternate layout order for every sample.
    source += f"    for sample in range({samples + 1}):\n"
    for kind in ["one", "two", "stream", "whole", "random"]:
        source += "        if sample % 2 == 0:\n"
        for layout, _ in layouts:
            source += f"            measure_{layout}_{kind}(&{layout}, sample)?\n"
        source += "        else:\n"
        for layout, _ in reversed(layouts):
            source += f"            measure_{layout}_{kind}(&{layout}, sample)?\n"
    return source + "    Ok(())\n"


def expected(rows, kind):
    total = 0
    for i in range(rows):
        n = i % 1024
        if kind == "one":
            total += n
        elif kind == "two":
            total += n + 1 if n % 2 == 0 else 0
        else:
            total += n * (n + 1) + (n + 2) * (n + 3) + (n + 4) * (n + 5) + (n + 6) * (n + 7)
    return total


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("compiler", type=Path, help="compiler built WITHOUT runtime-checks")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--rows", type=int, nargs="+", default=[8192, 262144])
    parser.add_argument("--repeats", type=int, default=20)
    parser.add_argument("--samples", type=int, default=7)
    parser.add_argument("--cpu", type=int)
    parser.add_argument("--native-clist", action="store_true", help="also measure the compiler-supported packed clist")
    args = parser.parse_args()
    assert args.repeats > 0 and args.samples > 0
    compiler = args.compiler.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "columns.plenty").write_text(generate((HERE / "particle.plenty").read_text()))
    report = {"compiler": str(compiler), "compiler_sha256": hashlib.sha256(compiler.read_bytes()).hexdigest(),
              "platform": platform.platform(), "cpu": args.cpu, "samples": args.samples,
              "repeats": args.repeats, "release": True, "native_clist": args.native_clist, "results": [], "construction": []}
    report["lscpu"] = subprocess.run(["lscpu"], text=True, capture_output=True, check=True).stdout
    env = {**os.environ, "PLENTY_CACHE": "off", "PLENTY_LINKER_THREADS": "1"}
    for rows in args.rows:
        assert rows > 0 and rows & (rows - 1) == 0, "permutation requires power-of-two row count"
        source = args.out / f"bench_{rows}.plenty"
        binary = args.out / f"bench_{rows}"
        source.write_text(program(rows, args.repeats, args.samples, args.native_clist))
        subprocess.run([str(compiler), "--release", "--compile", str(source), "-o", str(binary)], env=env, check=True)
        command = [str(binary.resolve())]
        if args.cpu is not None:
            command = ["taskset", "-c", str(args.cpu), *command]
        result = subprocess.run(command, env=env, text=True, capture_output=True, check=True)
        (args.out / f"raw_{rows}.txt").write_text(result.stdout)
        timings = {}
        for line in result.stdout.splitlines():
            name, *values = line.split()
            if name.startswith("build_"):
                nanos, checksum = values
                assert int(checksum) == rows
                report["construction"].append({"rows": rows, "layout": name.removeprefix("build_"),
                                               "ns_per_row": int(nanos) / rows})
                print(rows, name, f"{int(nanos) / rows:.3f} ns/row")
                continue
            sample, nanos, checksum = values
            layout, kind = name.split("_")
            assert int(checksum) == expected(rows, kind) * args.repeats, line
            if int(sample):  # sample zero is warmup
                timings.setdefault((layout, kind), []).append(int(nanos) / (rows * args.repeats))
        for (layout, kind), values in timings.items():
            record = {"rows": rows, "layout": layout, "kernel": kind,
                      "median_ns_per_row": statistics.median(values), "samples_ns_per_row": values}
            report["results"].append(record)
            print(rows, layout, kind, f"{record['median_ns_per_row']:.3f} ns/row")
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
