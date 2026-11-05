#!/usr/bin/env python3
"""Build, verify and time the Benchmarks Game programs against Plenty.

See book/src/development/benchmarks-game.md for the workflow. Uses only the
Python standard library.

    python3 benchmarks/game/bench.py check
    python3 benchmarks/game/bench.py run --mode quick [BENCHMARK ...]
    python3 benchmarks/game/bench.py run --mode full --output results.json
    python3 benchmarks/game/bench.py compare BASELINE CURRENT
    python3 benchmarks/game/bench.py dump n-body --filter advance
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import platform
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

GAME_DIR = Path(__file__).resolve().parent
REPO = GAME_DIR.parent.parent
DEFAULT_WORK = REPO / "target" / "bench-game"
DEFAULT_CORES = "0,2,4,6,8,10,12,14"
DEFAULT_BASELINE = GAME_DIR / "baseline.json"
EMITTED_CODE_DOC = "book/src/development/emitted-code.md"
EMITTED_CODE_SKILL = ".agents/skills/emitted-code/SKILL.md"
SCHEMA = 1
MODES = ("quick", "full")
DEFAULT_REPEATS = {"quick": 3, "full": 3}


class BenchError(Exception):
    pass


# --- manifests ---------------------------------------------------------------


def load_manifests(names: list[str] | None = None) -> list[dict]:
    manifests = []
    for path in sorted(GAME_DIR.glob("*/benchmark.json")):
        manifest = json.loads(path.read_text())
        manifest["dir"] = path.parent
        manifests.append(manifest)
    if names:
        known = {m["name"]: m for m in manifests}
        missing = [n for n in names if n not in known]
        if missing:
            raise BenchError(f"unknown benchmark(s): {', '.join(missing)}; known: {', '.join(known)}")
        manifests = [known[n] for n in names]
    return manifests


def program_id(variant: str) -> str:
    return f"plenty-{variant}"


# --- building ----------------------------------------------------------------


def plenty_compiler(args) -> Path:
    path = Path(args.plenty) if args.plenty else REPO / "target" / "release" / "plenty"
    if not path.exists():
        raise BenchError(f"{path} not found; run `cargo build --locked --release` first or pass --plenty")
    return path


def run_checked(command: list[str], **kwargs) -> subprocess.CompletedProcess:
    result = subprocess.run(command, capture_output=True, text=True, **kwargs)
    if result.returncode != 0:
        raise BenchError(f"command failed ({result.returncode}): {' '.join(map(str, command))}\n{result.stderr}")
    return result


def build_reference(manifest: dict, work: Path) -> Path:
    reference = manifest["reference"]
    build = reference["build"]
    source = manifest["dir"] / reference["file"]
    out_dir = work / "bin"
    out_dir.mkdir(parents=True, exist_ok=True)
    output = out_dir / f"{manifest['name']}.reference"
    if build["kind"] == "cargo":
        target_dir = work / "cargo"
        env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir), RUSTFLAGS=build.get("rustflags", ""))
        run_checked(build["command"], cwd=manifest["dir"], env=env)
        shutil.copy2(target_dir / build["binary"], output)
        return output
    tool = build["command"][0]
    if shutil.which(tool) is None:
        raise BenchError(f"{tool} not found")
    command = [part.replace("{source}", str(source)).replace("{output}", str(output)) for part in build["command"]]
    run_checked(command)
    return output


def build_plenty(manifest: dict, variant: dict, work: Path, compiler: Path) -> Path:
    source = (manifest["dir"] / variant["file"]).resolve()
    out_dir = work / "bin"
    out_dir.mkdir(parents=True, exist_ok=True)
    output = out_dir / f"{manifest['name']}.{program_id(variant['variant'])}"
    run_checked([str(compiler), "--compile", str(source), "-o", str(output)])
    return output


# --- inputs and outputs ------------------------------------------------------


def fasta_input(size: int, work: Path, fasta_binary: Path | None) -> Path:
    """Return the fasta output for `size`, generating it once per work directory."""
    if size == 1000:
        return GAME_DIR / "fasta" / "expected" / "1000.txt"
    path = work / "inputs" / f"fasta-{size}.txt"
    if path.exists():
        return path
    if fasta_binary is None:
        raise BenchError("generating fasta input needs a built fasta reference or Plenty program")
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_suffix(".partial")
    with partial.open("wb") as out:
        subprocess.run([str(fasta_binary), str(size)], stdout=out, check=True)
    partial.rename(path)
    return path


def outputs_match(actual: bytes, expected: bytes, compare: str) -> bool:
    if compare == "exact":
        return actual == expected
    if compare.startswith("float:"):
        tolerance = float(compare.split(":", 1)[1])
        a, e = actual.split(), expected.split()
        if len(a) != len(e):
            return False
        for x, y in zip(a, e):
            try:
                if abs(float(x) - float(y)) > tolerance:
                    return False
            except ValueError:
                if x != y:
                    return False
        return True
    raise BenchError(f"unknown comparison {compare!r}")


def command_for(manifest: dict, binary: Path, size: int) -> list[str]:
    # Stdin benchmarks take the site's ignored "0" argument.
    if manifest["input"] == "fasta-stdin":
        return [str(binary), "0"]
    return [str(binary), str(size)]


# --- running -----------------------------------------------------------------


def pinned(command: list[str], cores: str | None) -> list[str]:
    if cores and shutil.which("taskset"):
        return ["taskset", "-c", cores, *command]
    return command


def timed_run(command: list[str], stdin: Path | None, stdout: Path) -> dict:
    # A child of this Python process inherits its peak RSS, so ru_maxrss from
    # wait4 never falls below the interpreter's size. GNU time forks from a
    # small process and reports the program's own peak.
    rss_file = stdout.with_suffix(".rss")
    gnu_time = Path("/usr/bin/time")
    if gnu_time.exists():
        command = [str(gnu_time), "-q", "-f", "%M", "-o", str(rss_file), *command]
    with open(stdin or os.devnull, "rb") as inp, stdout.open("wb") as out:
        start = time.perf_counter()
        process = subprocess.Popen(command, stdin=inp, stdout=out, stderr=subprocess.PIPE)
        _, status, usage = os.wait4(process.pid, 0)
        wall = time.perf_counter() - start
        stderr = process.stderr.read().decode(errors="replace")
        process.stderr.close()
        process.returncode = os.waitstatus_to_exitcode(status)
    if process.returncode != 0:
        raise BenchError(f"{' '.join(command)} exited {process.returncode}\n{stderr}")
    max_rss = None
    if rss_file.exists():
        max_rss = int(rss_file.read_text().split()[-1])
        rss_file.unlink()
    return {
        "wall_s": round(wall, 4),
        "user_s": round(usage.ru_utime, 4),
        "sys_s": round(usage.ru_stime, 4),
        "max_rss_kb": max_rss,
    }


def perf_counts(command: list[str], stdin: Path | None, work: Path) -> dict | None:
    if shutil.which("perf") is None:
        return None
    report = work / "perf.csv"
    perf = ["perf", "stat", "-x", ",", "-e", "instructions:u,cycles:u", "-o", str(report), "--", *command]
    with open(stdin or os.devnull, "rb") as inp:
        result = subprocess.run(perf, stdin=inp, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    if result.returncode != 0 or not report.exists():
        return None
    counts = {}
    for line in report.read_text().splitlines():
        fields = line.split(",")
        if len(fields) > 2 and fields[0].strip().isdigit():
            event = fields[2].split(":")[0]
            counts[event] = int(fields[0])
    return counts or None


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


# --- commands ----------------------------------------------------------------


def build_all(manifests: list[dict], work: Path, compiler: Path, with_reference: bool) -> dict:
    """Build every program; return {name: {program_id: path}}."""
    binaries: dict[str, dict[str, Path]] = {}
    for manifest in manifests:
        programs: dict[str, Path] = {}
        if with_reference and manifest["status"] == "ready":
            try:
                programs["reference"] = build_reference(manifest, work)
            except BenchError as error:
                print(f"{manifest['name']}: reference not built: {error}", file=sys.stderr)
        for variant in manifest["plenty"]:
            programs[program_id(variant["variant"])] = build_plenty(manifest, variant, work, compiler)
        binaries[manifest["name"]] = programs
    return binaries


def fasta_generator(binaries: dict) -> Path | None:
    programs = binaries.get("fasta", {})
    return programs.get("reference") or programs.get("plenty-idiomatic")


def verify_check_size(manifest: dict, programs: dict[str, Path], work: Path, fasta: Path | None) -> None:
    check = manifest["check"]
    expected = (manifest["dir"] / check["expected"]).read_bytes()
    stdin = fasta_input(check["size"], work, fasta) if manifest["input"] == "fasta-stdin" else None
    for name, binary in programs.items():
        with open(stdin or os.devnull, "rb") as inp:
            result = subprocess.run(command_for(manifest, binary, check["size"]), stdin=inp, capture_output=True)
        if result.returncode != 0 or not outputs_match(result.stdout, expected, check["compare"]):
            raise BenchError(f"{manifest['name']} {name}: output at size {check['size']} differs from {check['expected']}")


def cmd_check(args) -> int:
    work = Path(args.work)
    manifests = load_manifests(args.benchmarks)
    compiler = plenty_compiler(args)
    binaries = build_all(manifests, work, compiler, with_reference=not args.no_reference)
    fasta = fasta_generator(binaries)
    for manifest in manifests:
        if manifest["status"] != "ready":
            print(f"{manifest['name']}: blocked: {manifest['blocked_reason']}")
            continue
        verify_check_size(manifest, binaries[manifest["name"]], work, fasta)
        print(f"{manifest['name']}: ok ({', '.join(binaries[manifest['name']])})")
    return 0


def tool_version(command: list[str]) -> str | None:
    try:
        result = subprocess.run(command, capture_output=True, text=True)
    except OSError:
        return None
    return result.stdout.splitlines()[0].strip() if result.returncode == 0 and result.stdout else None


def machine_info(cores: str | None) -> dict:
    info = {"system": platform.system(), "release": platform.release(), "machine": platform.machine(), "pinned_cores": cores}
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                info["cpu"] = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    info["logical_cpus"] = os.cpu_count()
    governor = Path("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
    if governor.exists():
        info["governor"] = governor.read_text().strip()
    return info


def git_state() -> dict:
    head = tool_version(["git", "-C", str(REPO), "rev-parse", "HEAD"])
    status = subprocess.run(["git", "-C", str(REPO), "status", "--porcelain"], capture_output=True, text=True)
    return {"commit": head, "dirty": bool(status.stdout.strip())}


def cmd_run(args) -> int:
    work = Path(args.work)
    work.mkdir(parents=True, exist_ok=True)
    manifests = load_manifests(args.benchmarks)
    compiler = plenty_compiler(args)
    repeats = args.repeats or DEFAULT_REPEATS[args.mode]
    cores = None if args.no_pin else args.cores
    binaries = build_all(manifests, work, compiler, with_reference=not args.no_reference)
    if any(m["input"] == "fasta-stdin" for m in manifests) and "fasta" not in binaries:
        binaries.update(build_all(load_manifests(["fasta"]), work, compiler, with_reference=not args.no_reference))
    fasta = fasta_generator(binaries)

    results = {
        "schema": SCHEMA,
        "date": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "mode": args.mode,
        "repeats": repeats,
        **git_state(),
        "versions": {
            "plenty": tool_version([str(compiler), "--version"]),
            "clang": tool_version(["clang", "--version"]),
            "rustc": tool_version(["rustc", "--version"]),
            "cargo": tool_version(["cargo", "--version"]),
            "python": platform.python_version(),
        },
        "flags": {"plenty": f"{compiler.name} --compile FILE -o OUT"},
        "machine": machine_info(cores),
        "benchmarks": {},
    }

    for manifest in manifests:
        name = manifest["name"]
        size = manifest["sizes"][args.mode]
        build = manifest["reference"]["build"]
        entry = {
            "status": manifest["status"],
            "size": size,
            "site_size": manifest["sizes"]["site"],
            "reference_build": " ".join(build["command"]) + (f" (RUSTFLAGS={build['rustflags']!r})" if "rustflags" in build else ""),
            "programs": {},
        }
        results["benchmarks"][name] = entry
        if manifest["status"] != "ready":
            entry["blocked_reason"] = manifest["blocked_reason"]
            print(f"{name}: blocked: {manifest['blocked_reason']}")
            continue
        programs = binaries[name]
        verify_check_size(manifest, programs, work, fasta)
        stdin = fasta_input(size, work, fasta) if manifest["input"] == "fasta-stdin" else None
        reference_digest = None
        reference_output = None
        # Reference first, so each Plenty output is compared with it.
        for program, binary in sorted(programs.items(), key=lambda item: item[0] != "reference"):
            command = pinned(command_for(manifest, binary, size), cores)
            output = work / "out" / f"{name}.{program}.out"
            output.parent.mkdir(parents=True, exist_ok=True)
            runs = []
            for _ in range(repeats):
                runs.append(timed_run(command, stdin, output))
            verified = None
            if program == "reference":
                reference_digest = digest(output)
                reference_output = output.with_suffix(".expected")
                shutil.copyfile(output, reference_output)
            elif reference_output is not None:
                if manifest["check"]["compare"] == "exact":
                    verified = digest(output) == reference_digest
                else:
                    verified = outputs_match(output.read_bytes(), reference_output.read_bytes(), manifest["check"]["compare"])
                if not verified:
                    raise BenchError(f"{name} {program}: output at size {size} differs from the reference")
            walls = [r["wall_s"] for r in runs]
            record = {
                "median_s": round(statistics.median(walls), 4),
                "min_s": min(walls),
                "user_s": round(statistics.median(r["user_s"] for r in runs), 4),
                "max_rss_kb": max((r["max_rss_kb"] for r in runs if r["max_rss_kb"] is not None), default=None),
                "runs": walls,
                "threads": manifest["reference"]["threads"] if program == "reference" else next(
                    v["threads"] for v in manifest["plenty"] if program_id(v["variant"]) == program),
            }
            if verified is not None:
                record["matches_reference"] = verified
            if args.perf:
                counts = perf_counts(command, stdin, work)
                if counts:
                    record.update(counts)
            entry["programs"][program] = record
            print(f"{name:20} {program:18} median {record['median_s']:9.3f}s  min {record['min_s']:9.3f}s", flush=True)
        if "reference" in entry["programs"]:
            ref = entry["programs"]["reference"]["median_s"]
            for program, record in entry["programs"].items():
                if program != "reference" and ref > 0:
                    record["ratio_to_reference"] = round(record["median_s"] / ref, 3)
        for path in (work / "out").glob(f"{name}.*"):
            path.unlink()

    text = json.dumps(results, indent=2) + "\n"
    if args.output:
        Path(args.output).write_text(text)
        print(f"wrote {args.output}")
    else:
        sys.stdout.write(text)
    if args.compare:
        return compare(json.loads(Path(args.compare).read_text()), results, args.threshold, args.metric)
    return 0


def metric_value(record: dict, metric: str) -> float | None:
    return {"time": record.get("median_s"), "ratio": record.get("ratio_to_reference"), "instructions": record.get("instructions")}[metric]


def compare(baseline: dict, current: dict, threshold: float, metric: str) -> int:
    """Print changes beyond `threshold`; return 1 if any Plenty program regressed."""
    regressions = 0
    if baseline.get("mode") != current.get("mode"):
        print(f"warning: comparing {current.get('mode')} results with a {baseline.get('mode')} baseline")
    for name, entry in current["benchmarks"].items():
        base = baseline["benchmarks"].get(name)
        if base is None:
            print(f"{name}: not in baseline")
            continue
        if entry["status"] != base["status"]:
            print(f"{name}: status {base['status']} -> {entry['status']}")
        if base.get("size") != entry.get("size"):
            print(f"{name}: size {base.get('size')} -> {entry.get('size')}; skipped")
            continue
        for program, record in entry["programs"].items():
            old_record = base["programs"].get(program)
            if old_record is None or program == "reference":
                continue
            old, new = metric_value(old_record, metric), metric_value(record, metric)
            if not old or new is None:
                continue
            change = new / old - 1
            mark = "REGRESSION" if change > threshold else ("improved" if change < -threshold else "ok")
            regressions += mark == "REGRESSION"
            print(f"{name:20} {program:18} {metric} {old:>12} -> {new:>12} ({change:+.1%}) {mark}")
    return 1 if regressions else 0


def cmd_compare(args) -> int:
    baseline = json.loads(Path(args.baseline).read_text())
    current = json.loads(Path(args.current).read_text())
    return compare(baseline, current, args.threshold, args.metric)


def cmd_dump(args) -> int:
    """Write Plenty CLIF/assembly and reference disassembly for one benchmark."""
    work = Path(args.work)
    [manifest] = load_manifests([args.benchmark])
    compiler = plenty_compiler(args)
    variants = [v for v in manifest["plenty"] if v["variant"] == args.variant]
    if not variants:
        raise BenchError(f"{args.benchmark} has no Plenty variant {args.variant!r}")
    out = work / "dump" / args.benchmark
    plenty_dir = out / program_id(args.variant)
    if plenty_dir.exists():
        shutil.rmtree(plenty_dir)
    command = [str(compiler), "--dump", args.stages, str((manifest["dir"] / variants[0]["file"]).resolve()), "-o", str(plenty_dir)]
    if args.filter:
        command += ["--dump-filter", args.filter]
    run_checked(command)
    print(f"Plenty {args.stages}: {plenty_dir}")
    if manifest["status"] == "ready" and not args.no_reference and shutil.which("objdump"):
        reference = build_reference(manifest, work)
        listing = out / "reference.objdump.txt"
        with listing.open("w") as f:
            subprocess.run(["objdump", "-d", "--no-show-raw-insn", "-M", "intel", str(reference)], stdout=f, check=True)
        print(f"reference disassembly: {listing}")
    print(f"Next: follow {EMITTED_CODE_DOC} ({EMITTED_CODE_SKILL}) to isolate a kernel and attribute each extra instruction.")
    return 0


def cmd_list(args) -> int:
    for manifest in load_manifests(args.benchmarks):
        ref = manifest["reference"]
        variants = ", ".join(v["variant"] for v in manifest["plenty"]) or "-"
        status = manifest["status"] if manifest["status"] == "ready" else f"blocked: {manifest['blocked_reason']}"
        print(f"{manifest['name']:20} {ref['language']} #{ref['program']} ({ref['threads']} thr)  plenty: {variants:22} {status}")
    return 0


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--work", default=str(DEFAULT_WORK), help="build, input and output directory (default: %(default)s)")
    p.add_argument("--plenty", help="Plenty compiler (default: target/release/plenty)")
    sub = p.add_subparsers(dest="command", required=True)

    s = sub.add_parser("list", help="list benchmarks and their status")
    s.add_argument("benchmarks", nargs="*")
    s.set_defaults(func=cmd_list)

    s = sub.add_parser("check", help="build and verify every program at its check size")
    s.add_argument("benchmarks", nargs="*")
    s.add_argument("--no-reference", action="store_true", help="skip building the reference programs")
    s.set_defaults(func=cmd_check)

    s = sub.add_parser("run", help="verify, then time each program")
    s.add_argument("benchmarks", nargs="*")
    s.add_argument("--mode", choices=MODES, default="quick")
    s.add_argument("--repeats", type=int)
    s.add_argument("--cores", default=DEFAULT_CORES, help="taskset core list (default: %(default)s)")
    s.add_argument("--no-pin", action="store_true")
    s.add_argument("--no-reference", action="store_true")
    s.add_argument("--perf", action="store_true", help="add a perf stat run for instruction and cycle counts")
    s.add_argument("--output", help="write results JSON here instead of stdout")
    s.add_argument("--compare", help="baseline JSON to compare against")
    s.add_argument("--threshold", type=float, default=0.10)
    s.add_argument("--metric", choices=("time", "ratio", "instructions"), default="time")
    s.set_defaults(func=cmd_run)

    s = sub.add_parser("compare", help="compare two results files")
    s.add_argument("baseline")
    s.add_argument("current")
    s.add_argument("--threshold", type=float, default=0.10)
    s.add_argument("--metric", choices=("time", "ratio", "instructions"), default="time")
    s.set_defaults(func=cmd_compare)

    s = sub.add_parser("dump", help="write Plenty CLIF/asm and reference disassembly")
    s.add_argument("benchmark")
    s.add_argument("--variant", default="idiomatic")
    s.add_argument("--stages", default="clif-opt,asm")
    s.add_argument("--filter", help="only functions whose name contains this text")
    s.add_argument("--no-reference", action="store_true")
    s.set_defaults(func=cmd_dump)
    return p


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    os.environ.setdefault("TMPDIR", str(REPO / "target" / "tmp"))
    Path(os.environ["TMPDIR"]).mkdir(parents=True, exist_ok=True)
    try:
        return args.func(args)
    except BenchError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
