#!/usr/bin/env python3
"""Private PR886 compiler-profile adapter; not a latency or native-program runner.

plan writes an unauthorized, nonexecuting 16-job emit-object schedule. execute
requires --allow-profile and a parent-pinned canonical manifest SHA256. Neither
command builds compilers. No --release --emit-object combination is supported.

Canonical manifest: schema=1, purpose=measurement, adapter=ADAPTER,
quiet_machine_approved=true, builds={main,candidate} with the reviewed acceptance
BUILD_FIELDS, accepted_build_records={path,sha256}, native_evidence={root,
index_sha256,inventory_sha256,corpora_proof_sha256,report_sha256,review_sha256},
environment=machine_snapshot(), child_environment=BASE_ENV, comparison_pairs=
[["C","I"],["T","U"]], toolchain_files=the authenticated R19 toolchain_files. canonical_build() defines the exact
role-local mapping (raw commands are provenance, not cross-role match fields).
The parent, not this tool, supplies authorization and constructs that manifest.

Only digest/load_inventory are imported from the reviewed 830 acceptance seam.
All profiler data and mock tests are diagnostics, never build-cfg-derived totals.
"""

import argparse
import base64
import calendar
from datetime import datetime, timedelta
from decimal import Decimal
from fractions import Fraction
import json
import math
import os
from pathlib import Path
import re
import signal
import stat
import subprocess

from checked_default_acceptance import digest, load_inventory

ADAPTER = "checked-default-profile-v1"
BUILD_RECORD = Path("/tmp/plenty-checked-r19-profile-alloc-build-records.json")
BUILD_RECORD_SHA256 = "f42220c731fbccc2ab9bbf41055d9e63f509935ec680dee6e5f43ef6c13f7de1"
NATIVE_ROOT = Path("/home/caleb/.cache/plenty-checked-r20-validation")
INPUT_ROOT = NATIVE_ROOT / "inputs"
NATIVE_REPORT = Path("/tmp/plenty-checked-r20-native-validation.md")
NATIVE_REVIEW = Path("/tmp/plenty-checked-r20-native-evidence-review.md")
NATIVE_PINS = {
    "index_sha256": "ad88b8d62fbfe5fa78f17193d0d83750e16d7d2e6cbd5a2bb111c0a478247ffc",
    "inventory_sha256": "3f8811fdc4f4a437ef5eecd4f47fea4967e1fab7bc4e37e47e59c6a6e5c4c3e0",
    "corpora_proof_sha256": "5c8cf7a0d6f21cf4d0f5df1e693c3999b105c79f8b7367a27481dbade5791860",
    "report_sha256": "b8d5c008681ec8118e5bd34bf2a335f6d14b4b66db9097a903f25c4737869d0d",
    "review_sha256": "b5970fc9409292742b80318ea7f6d2180d002b8d310dd045c16c12ee698cce20",
}
SOURCE_PINS = {
    "main": "8e8fccd9ff45fa760f66bef0cdc3e061774d2781",
    "candidate": "830763c9cd7a91fe02e2d05c6ab315b29fd19da8",
}
BINARY_PINS = {
    "main": "a8362888e71bf126602c51348de9438a783e2a1d57c99c8d986371d887d325ae",
    "candidate": "9b56943c58eb60cf6c9de314758b2b219afb7586d2630a6fd55eacb2cfc9d027",
}
BASE_ENV = {"HOME": "/home/caleb", "PATH": "/usr/bin:/bin", "LC_ALL": "C"}
MATCH_FIELDS = ("compiler_profile", "cargo_features", "cargo_flags", "rustflags",
                "runtime_rustflags", "target", "toolchain", "native_linker")
# Both pinned roles: compiler_cli -> compile_file_to_object_with_options ->
# program_object -> Sources::Graph::compile -> frontend::lower -> emit_object
# -> object_module. Cache is disabled; main's guard is dropped on normal return.
# hotpath-macros 0.28.6 lib_on.rs uses module_path!()::fn, not display short names.
MINIMUM_PHASES = ("main", "plenty::codegen::program_object", "plenty::frontend::lower",
                  "plenty::codegen::emit_object", "plenty::codegen::object_module")
LIMITS = [
    "Exact counts are Rust allocator REQUESTS above mimalloc, including successful realloc new-size requests; not retained heap, RSS, or generated-runtime allocations.",
    "APPROXIMATE bytes: display-rounded above 1023 B; nominal conversion uses powers of 1024, not recovered exact bytes.",
    "Exclusive allocation attribution excludes nested measured guards on the same thread but includes unmeasured callees; worker-thread attribution is not a complete process census.",
    "Calls are phase invocations, NOT allocation counts. No overlapping cumulative parent/child sum or inferred bytes from calls/averages/percentiles.",
    "Timing is profiling-inclusive and overhead-affected, not latency or comparable to perf[] samples. No timing ratios are produced.",
    "Objects are unlinked normal/debug generated-code artifacts, not optimized executables. Native proof references qualify only designated checksum entrypoints, not every generated function.",
    "This bounded compiler-profile component does not waive the failed external fullgate or establish whole-proposal acceptance.",
]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def json_load(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, f"duplicate JSON key: {key}")
            result[key] = value
        return result
    def invalid(value):
        raise ValueError(f"nonfinite JSON value: {value}")
    return json.loads(raw, object_pairs_hook=unique, parse_constant=invalid)


def canonical_path(path):
    path = Path(path)
    require(path.is_absolute() and ".." not in path.parts, "absolute canonical path required")
    for part in (path, *path.parents):
        require(not part.is_symlink(), f"symlink path refused: {part}")
    require(path.resolve() == path, f"noncanonical path: {path}")
    return path


def file_identity(path, *, native_tool=None):
    path = canonical_path(path)
    recorded = None
    if native_tool is not None:
        # The category is not a caller-supplied path allowlist. Reauthenticate
        # the original record even on rechecks; manifests cannot grant it.
        tools = accepted_records()["native_tools"]
        require(type(native_tool) is str and native_tool in tools, "unknown authenticated native tool")
        recorded = tools[native_tool]["file"]
        require(str(path) == recorded["path"], "authenticated native tool path mismatch")
    s = path.stat()
    require(stat.S_ISREG(s.st_mode) and (s.st_nlink == 1 or recorded is not None),
            f"regular nonaliased file required: {path}")
    identity = {"path": str(path), "sha256": digest(path.read_bytes()), "size": s.st_size,
                "device": s.st_dev, "inode": s.st_ino, "mode": s.st_mode,
                "mtime_ns": s.st_mtime_ns, "uid": s.st_uid}
    after = canonical_path(path).stat()
    fields = ("st_dev", "st_ino", "st_mode", "st_size", "st_mtime_ns", "st_ctime_ns", "st_uid", "st_gid", "st_nlink")
    require(all(getattr(s, f) == getattr(after, f) for f in fields), f"file changed during identity capture: {path}")
    if recorded is not None:
        observed = {"path": str(path), "sha256": identity["sha256"], "size_bytes": s.st_size,
                    "device": s.st_dev, "inode": s.st_ino, "mode": oct(stat.S_IMODE(s.st_mode)),
                    "mtime_ns": s.st_mtime_ns}
        require(observed == recorded, f"authenticated native tool identity mismatch: {path}")
        # R19 has no historical nlink/ctime. Bind fresh observations, never
        # invent old values or silently rebaseline aliases between jobs.
        identity.update(native_tool=native_tool, build_record_sha256=BUILD_RECORD_SHA256,
                        nlink=s.st_nlink, ctime_ns=s.st_ctime_ns)
    return identity


def pinned_bytes(path, sha256):
    identity = file_identity(path)
    require(identity["sha256"] == sha256, f"pinned hash mismatch: {path}")
    return Path(path).read_bytes()


def machine_snapshot():
    """Read facts only; never set affinity, governor, ASLR or any host policy."""
    return {"uname": list(os.uname()), "uid": os.getuid(), "cpu_count": os.cpu_count(),
            "allowed_affinity": sorted(os.sched_getaffinity(0)),
            "cpuinfo_static_sha256": digest(b"\n".join(line for line in Path("/proc/cpuinfo").read_bytes().splitlines()
                                                       if not line.startswith((b"cpu MHz", b"bogomips")))),
            "boot_id": Path("/proc/sys/kernel/random/boot_id").read_text().strip()}


def canonical_build(records, role):
    b = records["builds"][role]
    command = b["command"]
    # Remove only the separately authenticated role-local Cargo target-dir pair.
    i = command.index("--target-dir")
    require(command[i + 1] == b["target_dir"] and command.count("--target-dir") == 1,
            "unexpected Cargo target-dir provenance")
    flags = command[1:i] + command[i + 2:]
    return {"source_sha": b["source_sha"], "source_tree": b["source_path"],
            "binary": b["frozen_compiler"]["path"],
            "binary_sha256": b["frozen_compiler"]["sha256"], "frozen_snapshot": True,
            "compiler_profile": b["compiler_profile"], "cargo_features": b["features"],
            "cargo_flags": flags, "rustflags": b["rustflags"],
            "runtime_rustflags": b["runtime_rustflags"], "target": b["target"],
            "toolchain": records["toolchain"]["rustc_vV"] + "\n" + records["toolchain"]["cargo_vV"],
            "native_linker": records["native_tools"]["clang"]["version"] + "\n" + records["native_tools"]["mold"]["version"],
            "runtime_archive_sha256": b["runtime_archives"][0]["sha256"],
            "build_command": command}


def accepted_records():
    records = json_load(pinned_bytes(BUILD_RECORD, BUILD_RECORD_SHA256))
    require(records["schema"] == 1 and records["status"] == "PASS", "unaccepted builds")
    for role in SOURCE_PINS:
        b = records["builds"][role]
        require(b["source_sha"] == SOURCE_PINS[role] and b["frozen_compiler"]["sha256"] == BINARY_PINS[role],
                "wrong frozen pair")
        require(b["status"] == "PASS" and b["exit_code"] == 0 and b["final_status"] == "" and b["staged_files"] == [],
                "unclean/failed original build")
        require(b["features"] == ["profile", "profile-alloc"] and b["compiler_profile"] == "release",
                "not a release profile-alloc build")
    return records


def load_profile_manifest(path, sha256):
    raw = pinned_bytes(path, sha256)
    manifest = json_load(raw)
    require(type(manifest.get("schema")) is int and manifest["schema"] == 1 and manifest.get("adapter") == ADAPTER,
            "profile manifest schema/adapter required")
    require(manifest.get("purpose") == "measurement" and manifest.get("quiet_machine_approved") is True,
            "measurement purpose and genuine quiet approval required")
    require(manifest["accepted_build_records"] == {"path": str(BUILD_RECORD), "sha256": BUILD_RECORD_SHA256},
            "accepted build-record identity required")
    require(manifest["native_evidence"] == {"root": str(NATIVE_ROOT), **NATIVE_PINS},
            "original accepted native identities required")
    records = accepted_records()
    for role in SOURCE_PINS:
        expected = canonical_build(records, role)
        require(all(manifest["builds"][role].get(k) == v for k, v in expected.items()) and
                manifest["builds"][role].get("frozen_snapshot") is True,
                f"{role} fields differ from OWN authenticated build record")
    main, candidate = (manifest["builds"][r] for r in SOURCE_PINS)
    require(all(main[k] == candidate[k] for k in MATCH_FIELDS), "semantic build settings differ")
    require(manifest["comparison_pairs"] == [["C", "I"], ["T", "U"]], "only complete C->I and T->U comparison pairs supported")
    require(manifest["toolchain_files"] == records["toolchain_files"], "toolchain identities changed")
    require(manifest["child_environment"] == BASE_ENV, "unsupported child environment")
    require(manifest["environment"] == machine_snapshot(), "actual machine/environment mismatch")
    return manifest, records


def native_qualifiers(inventory):
    """Import ONLY exact reviewed R20 rows, anchored by the original index hash."""
    index = json_load(pinned_bytes(NATIVE_ROOT / "artifact-index.json", NATIVE_PINS["index_sha256"]))
    indexed = {}
    for row in index:
        require(row["path"] not in indexed, "duplicate native index path")
        indexed[row["path"]] = row["sha256"]
    pinned_bytes(NATIVE_REPORT, NATIVE_PINS["report_sha256"])
    pinned_bytes(NATIVE_REVIEW, NATIVE_PINS["review_sha256"])
    proof = json_load(pinned_bytes(NATIVE_ROOT / "corpora-proof.json", NATIVE_PINS["corpora_proof_sha256"]))
    def read_indexed(path):
        require(str(path) in indexed, f"native artifact not indexed: {path}")
        return pinned_bytes(path, indexed[str(path)])
    raw = read_indexed(NATIVE_ROOT / "corpora/results.jsonl")
    rows = [json_load(line) for line in raw.splitlines()]
    source_identities = {e["source"]: file_identity(INPUT_ROOT / e["source"]) for e in corpus_entries(inventory)}
    executable_identities = {}
    environment = json_load(read_indexed(NATIVE_ROOT / "corpora/environment.json"))
    runtime = [r for r in rows if r["phase"] == "runtime"]
    require([{**row, "group": "corpora"} for row in runtime] == proof,
            "native proof differs from original runtime rows plus recorded group")
    selected = corpus_entries(inventory)
    expected = {(e["source"], p, phase) for e in selected for p in ("debug", "release")
                for phase in ("check", "build", "runtime")}
    actual = set()
    for row in rows:
        key = (row["source"], row["profile"], row["phase"])
        require(key in expected and key not in actual, "native qualification incomplete/duplicate")
        actual.add(key)
        require(row["returncode"] == 0 and row["timeout"] is False and row["spawn_error"] is None,
                "failed native qualifier")
        require(base64.b64decode(row["stdout_base64"], validate=True) == row["stdout"].encode() and
                base64.b64decode(row["stderr_base64"], validate=True) == row["stderr"].encode(),
                "native raw stream mismatch")
        if row["phase"] == "runtime":
            entry = next(e for e in selected if e["source"] == row["source"])
            executable = NATIVE_ROOT / "corpora" / entry["mode"] / row["profile"] / Path(row["source"]).stem
            require(row["command"] == [str(executable)] and row["case"] == "checksum" and
                    row["stdout"] == entry["cases"][0]["stdout"] and row["stderr"] == "",
                    "native designated entrypoint/checksum mismatch")
            read_indexed(executable)  # Same output from a replaced executable is not proof.
            executable_identities[str(executable)] = file_identity(executable)
    require(actual == expected, "missing native qualification")
    return {"kind": "reviewed-untimed-reference-outside-profile-interval",
            "identities": NATIVE_PINS, "rows": rows, "environment": environment,
            "source_identities": source_identities, "executable_identities": executable_identities,
            "limits": "Designated checksum entrypoints only; not all generated functions."}


def corpus_entries(inventory):
    expected = {f"sources/{mode}/corpus-{units}.plenty" for mode in "TCIU" for units in (1000, 4000)}
    entries = [e for e in inventory["entries"] if e["source"] in expected]
    require({e["source"] for e in entries} == expected and len(entries) == 8,
            "full unchanged 1000/4000 T/C/I/U corpora required")
    for e in entries:
        units = int(Path(e["source"]).stem.split("-")[1])
        require(e["mode"] == Path(e["source"]).parent.name and e["cases"] == [{
            "args": [], "expected": "success", "name": "checksum",
            "stdout": f"{(units - 1) ** 2 + 6 * (units - 1) + 62}\n"}], "corpus oracle changed")
    return sorted(entries, key=lambda e: ("TCIU".index(e["mode"]), e["source"]))


def accepted_inventory():
    canonical_path(INPUT_ROOT)
    pinned_bytes(INPUT_ROOT / "inventory.json", NATIVE_PINS["inventory_sha256"])
    inventory = load_inventory(INPUT_ROOT)
    aliases = set()
    for e in inventory["entries"]:
        identity = file_identity(INPUT_ROOT / e["source"])
        alias = (identity["device"], identity["inode"])
        require(alias not in aliases, "duplicate input alias")
        aliases.add(alias)
    corpus_entries(inventory)
    return inventory


def profile_environment(job, root):
    # env-i semantics: no ambient HOTPATH*, PLENTY_PROFILE* (even '=0'),
    # loader/preload controls, upload settings, Rust wrappers, or cache controls.
    return {**BASE_ENV, "PLENTY_CACHE": "off", "PLENTY_CACHE_DIR": str(root / "cache"),
            "PLENTY_PROFILE": "1", "HOTPATH_METRICS_SERVER_OFF": "true",
            "HOTPATH_REPORT": "functions-alloc,functions-timing", "HOTPATH_OUTPUT_FORMAT": "json",
            "HOTPATH_OUTPUT_PATH": job["report"], "HOTPATH_ALLOC_METRIC": job["metric"],
            "HOTPATH_ALLOC_CUMULATIVE": "false", "HOTPATH_EXCLUDE_WRAPPER": "false",
            "HOTPATH_FUNCTIONS_LIMIT": "0", "HOTPATH_TIME_SAMPLING_RATE": "1",
            "HOTPATH_FUNCTIONS_TIME_SAMPLING_RATE": "1", "HOTPATH_REPORT_LABEL": job["id"]}


def make_plan(inventory, builds, root):
    root = canonical_path(root)
    jobs = []
    for entry in corpus_entries(inventory):
        role = "main" if entry["mode"] in "TC" else "candidate"
        for metric in ("count", "bytes"):
            name = f"{entry['mode']}-{Path(entry['source']).stem}-emit-object-{metric}"
            job = {"id": name, "mode": entry["mode"], "role": role, "metric": metric,
                   "phase": "emit-object", "generated_profile": "normal-debug-not-optimized",
                   "source": str(INPUT_ROOT / entry["source"]), "source_sha256": entry["sha256"],
                   "source_version": builds[role]["source_sha"],
                   "binary_sha256": builds[role]["binary_sha256"], "minimum_phases": list(MINIMUM_PHASES)}
            job.update({key: str(root / (name + suffix)) for key, suffix in
                        (("report", ".report.json"), ("object", ".o"), ("stdout", ".stdout"), ("stderr", ".stderr"))})
            job["command"] = [builds[role]["binary"], "--emit-object", job["source"], "-o", job["object"]]
            job["environment"] = profile_environment(job, root)
            jobs.append(job)
    paths = [j[k] for j in jobs for k in ("report", "object", "stdout", "stderr")]
    require(len(set(paths)) == len(paths), "duplicate profile output")
    return {"schema": 1, "adapter": ADAPTER, "status": "plan-only-NOT-executed-NOT-authorized",
            "inventory_sha256": NATIVE_PINS["inventory_sha256"], "jobs": jobs, "limits": LIMITS}


def uint(value, field):
    require(type(value) is int and 0 <= value <= (1 << 64) - 1, f"invalid {field}")
    return value


def count_value(value):
    require(isinstance(value, str) and re.fullmatch(r"0|[1-9][0-9]*", value), "count must be EXACT decimal integer string")
    return uint(int(value), "allocation count")


def byte_value(value):
    # output.rs format_bytes uses 1024 with B, KB, MB, GB, TB (not SI).
    require(isinstance(value, str), "bytes must be display string")
    m = re.fullmatch(r"(0|[1-9][0-9]*)(?:\.([0-9]))? (B|KB|MB|GB|TB)", value)
    require(m is not None, "malformed approximate byte display")
    unit = m[3]
    require((unit == "B") == (m[2] is None), "wrong byte display precision")
    amount = Decimal(value.split()[0])
    require(amount * 1024 ** ("B", "KB", "MB", "GB", "TB").index(unit) <= Decimal(1 << 64), "byte display out of range")
    require(unit != "B" or amount <= 1023, "non-display exact byte form refused")
    require(unit == "B" or amount >= 1, "invalid scaled byte display")
    return {"raw": value, "precision": "APPROXIMATE", "nominal_bytes_approx": str(amount * 1024 ** ("B", "KB", "MB", "GB", "TB").index(unit))}


def byte_interval(value):
    """Conservative display-rounding bounds, NOT recovered exact byte totals."""
    byte_value(value)
    amount, unit = value.split()
    factor = 1024 ** ("B", "KB", "MB", "GB", "TB").index(unit)
    nominal = Decimal(amount) * factor
    radius = Decimal("0.05") * factor if unit != "B" else 0
    return max(Decimal(0), nominal - radius), nominal + radius


def rounded_integer_bounds(nominal, radius, factor, division_rounds=False):
    """Conservative u64 bounds, not an inverse of hotpath's display formatter.

    output.rs casts u64 to f64, scales, then rounds to a display step. Each
    nearest-even binary cast/division contributes at most half an ULP; scaling
    bytes by powers of two is exact. First bound the possible input magnitude
    with the global u64 error, then use its local ULPs (not a global tolerance).
    Decimal formatting ties stay inclusive; integer division below is strict.
    """
    maximum = (1 << 64) - 1
    nominal, radius = Fraction(nominal), Fraction(radius)
    def binary_error(upper):
        converted = float(upper)
        cast_error = Fraction.from_float(math.ulp(converted)) / 2
        division_error = (Fraction.from_float(math.ulp(converted / factor)) * factor / 2
                          if division_rounds else 0)
        return cast_error + division_error
    upper = min(maximum, nominal + radius + binary_error(maximum))
    error = binary_error(upper)
    lo = max(0, math.ceil(nominal - radius - error))
    hi = min(maximum, math.floor(nominal + radius + error))
    require(lo <= hi, "display outside producer u64 range")
    return lo, hi


def byte_integer_bounds(value):
    lo, hi = byte_interval(value)
    if value.endswith(" B"):
        return int(lo), int(hi)
    factor = 1024 ** ("B", "KB", "MB", "GB", "TB").index(value.split()[1])
    return rounded_integer_bounds((lo + hi) / 2, (hi - lo) / 2, factor)


def duration_value(value):
    # output.rs: exact integer ns below 1000, otherwise two decimal places.
    require(isinstance(value, str) and re.fullmatch(r"(?:0|[1-9][0-9]*)(?: ns|\.[0-9]{2} (?:µs|ms|s))", value),
            "invalid profiling duration")
    amount, unit = value.split()
    if unit == "ns":
        require(int(amount) < 1000, "non-display exact duration form refused")
        return int(amount), int(amount)
    factor = {"µs": 1000, "ms": 1000000, "s": 1000000000}[unit]
    lo, hi = rounded_integer_bounds(Decimal(amount) * factor, Decimal("0.005") * factor,
                                    factor, division_rounds=True)
    # Unit selection happens BEFORE decimal rounding: 1000.00 µs can be real.
    lo = max(lo, factor)
    hi = min(hi, factor * 1000 - 1) if unit != "s" else hi
    require(lo <= hi, "duration outside producer display band")
    return lo, hi


def require_integer_average(total_bounds, average_bounds, samples, message):
    # alloc/state.rs: avg_bytes/avg_duration_ns truncate an integer total/count.
    # There must be an integer total T and average A with A*n <= T < (A+1)*n.
    total_lo, total_hi = total_bounds
    avg_lo, avg_hi = average_bounds
    require(total_hi >= avg_lo * samples and total_lo <= (avg_hi + 1) * samples - 1, message)


def validate_meta(meta):
    require(isinstance(meta, dict), "missing report meta")
    require(isinstance(meta.get("rustc"), str) and isinstance(meta.get("os"), str),
            "meta rustc/os must be strings")
    value = meta.get("created_at")
    require(isinstance(value, str), "meta created_at must be RFC3339 string")
    match = re.fullmatch(r"([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})", value)
    require(match is not None, "invalid meta created_at RFC3339 timestamp")
    year, month, day, hour, minute, second = map(int, match.groups()[:6])
    offset = match[7]
    offset_hour, offset_minute = (0, 0) if offset in ("Z", "z") else (int(offset[1:3]), int(offset[4:6]))
    require(offset_hour < 24 and offset_minute < 60 and second <= 60,
            "invalid meta created_at RFC3339 component")
    # A Gregorian 400-year cycle also validates RFC3339 year0000, which Python's
    # datetime cannot represent. Optional metadata and the timestamp stay raw.
    try:
        local = datetime(400 + year % 400, month, day, hour, minute, min(second, 59))
    except ValueError as error:
        raise ValueError("invalid meta created_at RFC3339 calendar/time") from error
    if second == 60:
        # time0.3.55 RFC3339 parsing permits leap seconds only at UTC month end.
        sign = -1 if offset.startswith("-") else 1
        utc = local - timedelta(minutes=sign * (offset_hour * 60 + offset_minute))
        require((utc.hour, utc.minute) == (23, 59) and utc.day == calendar.monthrange(utc.year, utc.month)[1],
                "invalid meta created_at RFC3339 leap second")


def phase_locations(role):
    return {"plenty::codegen::program_object": {"file": "src/codegen.rs", "line": 338, "column": 4},
            "plenty::codegen::emit_object": {"file": "src/codegen.rs", "line": 690, "column": 4},
            "plenty::codegen::object_module": {"file": "src/codegen.rs", "line": 735, "column": 4},
            "plenty::frontend::lower": {"file": "src/frontend.rs", "line": 4732 if role == "main" else 4662, "column": 4}}


def parse_report(raw, metric, label, minimum_phases=MINIMUM_PHASES, expected_locations=None):
    require(metric in ("count", "bytes"), "unsupported expected metric")
    report = json_load(raw)
    require(isinstance(report, dict), "report must be JSON object")
    require(report.get("type") == "hotpath_report" and report.get("version") == "0.28.6", "wrong hotpath type/version")
    require(report.get("label") == label, "wrong report label")
    validate_meta(report.get("meta"))
    rates = report.get("time_sampling")
    require(rates is None or (isinstance(rates, dict) and all(type(v) in (int, float) and v == 1 for v in rates.values())),
            "sampled profile not supported")
    sections = {}
    for section, mode in (("functions_alloc", "alloc-" + metric), ("functions_timing", "timing")):
        data = report.get(section)
        require(isinstance(data, dict) and data.get("profiling_mode") == mode, "missing/confused metric section")
        require(data.get("caller_name") == "main" and data.get("percentiles") == [50.0, 95.0], "wrong guard configuration")
        expected_description = (f"Exclusive allocation {metric} by each function (excluding nested calls)."
                                if section == "functions_alloc" else "Function execution time metrics.")
        require(data.get("description") == expected_description, "wrong attribution mode")
        uint(data.get("total_elapsed_ns"), "elapsed_ns")
        duration_value(data.get("time_elapsed"))
        rows = data.get("data")
        require(isinstance(rows, list) and len(rows) > 0 and
                uint(data.get("included_count"), "included_count") == len(rows) == uint(data.get("total_count"), "total_count"),
                "partial/truncated profile rows")
        parsed, ids = {}, set()
        for row in rows:
            require(isinstance(row, dict), "phase row must be JSON object")
            name = row.get("name")
            require(isinstance(name, str) and (name == "main" or re.fullmatch(r"[A-Za-z_][A-Za-z_0-9]*(?:::[A-Za-z_][A-Za-z_0-9]*)+", name)),
                    "missing/short phase name")
            require(name not in parsed and uint(row.get("id"), "phase id") not in ids and row["id"] < (1 << 32), "duplicate/invalid phase identity")
            ids.add(row["id"])
            calls = uint(row.get("calls"), "phase invocations")
            require(calls > 0 and uint(row.get("sampled_calls"), "sampled invocations") == calls,
                    "inconsistent calls/sampling")
            if name != "main":
                loc = row.get("location")
                require(isinstance(loc, dict) and isinstance(loc.get("file"), str) and loc["file"] and
                        uint(loc.get("line"), "location line") > 0 and uint(loc.get("column"), "location column") > 0,
                        "missing phase source location")
            require(isinstance(row.get("percent_total"), str) and re.fullmatch(r"(?:0|[1-9][0-9]*)\.[0-9]{2}%", row["percent_total"]) and
                    (section == "functions_timing" or Decimal(row["percent_total"][:-1]) <= 100), "invalid phase percent")
            values = [row.get(k) for k in ("total", "avg", "p50", "p95")]
            if section == "functions_alloc":
                if metric == "count":
                    total, avg, _, _ = [count_value(v) for v in values]
                    require(avg == total // calls, "inconsistent count average/calls")
                    allocation = {"total_count_exact": total, "precision": "EXACT"}
                else:
                    converted = [byte_value(v) for v in values]
                    allocation = converted[0]
                    require_integer_average(byte_integer_bounds(row["total"]), byte_integer_bounds(row["avg"]), calls,
                                            "inconsistent approximate total/average/calls")
            else:
                total_bounds, avg_bounds, _, _ = [duration_value(v) for v in values]
                require_integer_average(total_bounds, avg_bounds, row["sampled_calls"],
                                        "inconsistent profiling duration total/average/sampled_calls")
                allocation = {"precision": "profiling-inclusive-overhead-affected", "total_raw": row["total"],
                              "display_precision": "APPROXIMATE-rounded-duration; conservative bounds, not recovered raw timing"}
            parsed[name] = {"raw": row, "value": allocation, "calls_meaning": "phase invocations, not allocations"}
        require(set(minimum_phases) <= parsed.keys(), "missing minimum emit-object phases")
        if expected_locations is not None:
            require(all(parsed[k]["raw"].get("location") == v for k, v in expected_locations.items()),
                    "minimum phase location differs from pinned source")
        if section == "functions_alloc":
            if metric == "count":
                grand = count_value(data.get("total_allocated"))
                require(grand == sum(v["value"]["total_count_exact"] for v in parsed.values()),
                        "inconsistent exclusive count total")
            else:
                byte_value(data.get("total_allocated"))
                grand_lo, grand_hi = byte_integer_bounds(data["total_allocated"])
                intervals = [byte_integer_bounds(row["raw"]["total"]) for row in parsed.values()]
                require(grand_hi >= sum(lo for lo, _ in intervals) and grand_lo <= sum(hi for _, hi in intervals),
                        "inconsistent approximate exclusive total")
        sections[section] = parsed
    alloc, timing = sections.values()
    require(alloc.keys() == timing.keys(), "partial alloc/timing phase sets")
    require(all(alloc[k]["raw"]["calls"] == timing[k]["raw"]["calls"] and
                alloc[k]["raw"]["id"] == timing[k]["raw"]["id"] and
                alloc[k]["raw"].get("location") == timing[k]["raw"].get("location") for k in alloc),
            "inconsistent alloc/timing phase identities")
    return {"metric": metric, "version": "0.28.6", "sections": sections, "raw": report, "limits": LIMITS}


def compare_reports(before, after):
    require(before["metric"] == after["metric"] and before["version"] == after["version"], "incompatible report comparison")
    a, b = (r["sections"]["functions_alloc"] for r in (before, after))
    common = sorted(a.keys() & b.keys())
    def ratio(name):
        if before["metric"] == "count":
            numerator, denominator = (r[name]["value"]["total_count_exact"] for r in (b, a))
            return {"precision": "EXACT-count-fraction", "after_over_before": f"{numerator}/{denominator}" if denominator else None}
        numerator, denominator = (Decimal(r[name]["value"]["nominal_bytes_approx"]) for r in (b, a))
        return {"precision": "APPROXIMATE-byte-ratio", "after_over_before": str(numerator / denominator) if denominator else None}
    return {"metric": before["metric"], "common_phases": [
        {"name": k, "before": a[k], "after": b[k], "ratio": ratio(k)} for k in common],
        "removed_phases": {k: a[k] for k in sorted(a.keys() - b.keys())},
        "added_phases": {k: b[k] for k in sorted(b.keys() - a.keys())},
        "ratio_limits": "After/before common phase allocation values only; zero denominator unavailable. No missing-phase or timing ratios."}


def write_new(path, data):
    with Path(path).open("xb") as f:
        f.write(data)


def write_json_new(path, value):
    write_new(path, (json.dumps(value, indent=2, sort_keys=True) + "\n").encode())


def new_root(path):
    path = canonical_path(path)
    require(not os.path.lexists(path), "output root already exists")
    parent = path.parent.stat()
    require(parent.st_uid == os.getuid() and not parent.st_mode & 0o022, "output parent must be owned and non-writable by others")
    path.mkdir(mode=0o700)
    return path


def output_guard(root, paths):
    s = canonical_path(root).stat()
    require(s.st_uid == os.getuid() and stat.S_IMODE(s.st_mode) == 0o700, "output root is not private/owned")
    for path in paths:
        path = canonical_path(path)
        require(path.parent == root and not os.path.lexists(path), "output must be unique/new directly inside owned root")


def run_child(command, environment, root, name, timeout):
    stdout, stderr = (root / (name + suffix) for suffix in (".stdout", ".stderr"))
    output_guard(root, (stdout, stderr))
    row = {"command": command, "environment": environment, "cwd": str(root),
           "timeout_seconds_safety_only": timeout, "returncode": None, "timeout": False,
           "spawn_error": None, "status": "blocked"}
    # File-backed raw streams survive timeout and avoid a grandchild holding a pipe.
    with stdout.open("xb") as out, stderr.open("xb") as err:
        try:
            child = subprocess.Popen(command, cwd=root, env=environment, stdin=subprocess.DEVNULL,
                                     stdout=out, stderr=err, start_new_session=True)
            try:
                row["returncode"] = child.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                row["timeout"] = True
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                row["returncode"] = child.wait()
        except OSError as error:
            row["spawn_error"] = str(error)
    for key, path in (("stdout", stdout), ("stderr", stderr)):
        raw = path.read_bytes()
        row[key] = raw.decode("utf-8", errors="replace")
        row[key + "_base64"] = base64.b64encode(raw).decode("ascii")
        row[key + "_sha256"] = digest(raw)
    row["status"] = "passed" if row["returncode"] == 0 and not row["timeout"] and row["spawn_error"] is None else "blocked"
    return row


def protected_files(manifest_path, manifest_sha256, records, inventory):
    files = {str(manifest_path): manifest_sha256, str(BUILD_RECORD): BUILD_RECORD_SHA256,
             str(INPUT_ROOT / "inventory.json"): NATIVE_PINS["inventory_sha256"],
             str(NATIVE_ROOT / "artifact-index.json"): NATIVE_PINS["index_sha256"],
             str(NATIVE_ROOT / "corpora-proof.json"): NATIVE_PINS["corpora_proof_sha256"],
             str(NATIVE_REPORT): NATIVE_PINS["report_sha256"], str(NATIVE_REVIEW): NATIVE_PINS["review_sha256"]}
    files.update({str(INPUT_ROOT / e["source"]): e["sha256"] for e in inventory["entries"]})
    for b in records["builds"].values():
        frozen = file_identity(b["frozen_compiler"]["path"])
        require(stat.S_IMODE(frozen["mode"]) == 0o555, "frozen compiler must retain mode 0555")
        for row in [b["frozen_compiler"], *b["inputs"], *b["runtime_archives"]]:
            files[row["path"]] = row["sha256"]
    # Tool paths stay literal; no executable-path string substitution is allowed.
    for row in records["toolchain_files"]:
        files[row["path"]] = row["sha256"]
    identities = {path: file_identity(path) for path in files}
    require(all(identities[p]["sha256"] == h for p, h in files.items()), "protected input changed")
    require(records["native_tools"] == accepted_records()["native_tools"], "unauthenticated native tool records")
    for name, tool in records["native_tools"].items():
        path = tool["file"]["path"]
        require(path not in identities, "native tool overlaps protected input")
        identities[path] = file_identity(path, native_tool=name)
    return identities


def check_integrity(before):
    require(before == {p: file_identity(p, native_tool=identity.get("native_tool"))
                       for p, identity in before.items()}, "post-run protected-file integrity failed")


def source_qualifiers(manifest, root, prefix):
    rows = []
    for role in SOURCE_PINS:
        tree = manifest["builds"][role]["source_tree"]
        canonical_path(tree)
        for label, args in (("pin", ["rev-parse", "HEAD"]), ("clean", ["status", "--porcelain", "--untracked-files=all"]),
                            ("staged", ["diff", "--cached", "--name-only"])):
            row = run_child(["/usr/bin/git", "-C", tree, *args], BASE_ENV, root, f"{prefix}-{role}-{label}", 30)
            rows.append(row)
            write_json_new(root / f"{prefix}-{role}-{label}.json", row)
            require(row["status"] == "passed", "source qualifier failed")
            expected = SOURCE_PINS[role] + "\n" if label == "pin" else ""
            require(row["stdout"] == expected and row["stderr"] == "", "source pin/clean/staged mismatch")
    return rows


def execute(manifest_path, manifest_sha256, output, allow_profile=False, timeout=300):
    require(allow_profile is True, "explicit --allow-profile required")
    require(type(timeout) in (int, float) and math.isfinite(timeout) and timeout > 0, "positive finite safety timeout required")
    manifest_path = canonical_path(manifest_path)
    manifest, records = load_profile_manifest(manifest_path, manifest_sha256)
    inventory = accepted_inventory()
    qualifiers = native_qualifiers(inventory)
    before = protected_files(manifest_path, manifest_sha256, records, inventory)
    before.update(qualifiers["executable_identities"])
    root = new_root(output)
    result = {"schema": 1, "adapter": ADAPTER, "status": "blocked", "jobs": [], "limits": LIMITS}
    try:
        plan = make_plan(inventory, manifest["builds"], root)
        write_json_new(root / "plan.json", plan)
        write_new(root / "manifest.json", manifest_path.read_bytes())
        write_new(root / "inventory.json", (INPUT_ROOT / "inventory.json").read_bytes())
        write_json_new(root / "native-qualifiers.json", qualifiers)
        source_qualifiers(manifest, root, "pre")
        for role in SOURCE_PINS:
            tree = Path(manifest["builds"][role]["source_tree"])
            row = run_child(["/usr/bin/git", "-C", str(tree), "ls-files", "-z", "--", "src", "stdlib", "plenty-runtime", "Cargo.toml", "Cargo.lock", "build.rs", ".cargo"], BASE_ENV, root, f"pre-{role}-production-files", 30)
            write_json_new(root / f"pre-{role}-production-files.json", row)
            require(row["status"] == "passed" and row["stderr"] == "", "production source inventory failed")
            names = base64.b64decode(row["stdout_base64"]).split(b"\0")
            require(names[-1] == b"" and len(names) > 1, "empty/malformed production inventory")
            for name in names[:-1]:
                path = canonical_path(tree / os.fsdecode(name))
                require(path.is_relative_to(tree), "production source escaped worktree")
                before[str(path)] = file_identity(path)
        write_json_new(root / "protected-before.json", before)
        owned = {str(p): file_identity(p) for p in root.iterdir() if p.is_file()}
        for job in plan["jobs"]:
            check_integrity(before)
            check_integrity(owned)
            actual_environment = machine_snapshot()
            require(manifest["environment"] == actual_environment, "machine/environment drift")
            output_guard(root, [job[k] for k in ("report", "object", "stdout", "stderr")])
            row = {**job, "status": "blocked", "actual_environment_before": actual_environment}
            try:
                row["process"] = run_child(job["command"], job["environment"], root, job["id"], timeout)
                # Preserve partial raw reports/objects even when child status failed.
                for key in ("report", "object"):
                    path = Path(job[key])
                    if os.path.lexists(path):
                        row[key + "_identity"] = file_identity(path)
                        if key == "report":
                            row["report_base64"] = base64.b64encode(path.read_bytes()).decode("ascii")
                require(row["process"]["status"] == "passed", "compiler failed/spawn error/timeout; no retry")
                parsed = parse_report(Path(job["report"]).read_bytes(), job["metric"], job["id"], expected_locations=phase_locations(job["role"]))
                row["parsed_report"] = parsed
                blob = Path(job["object"]).read_bytes()
                require(len(blob) >= 64 and blob[:6] == b"\x7fELF\x02\x01" and blob[16:20] == b"\x01\x00\x3e\x00",
                        "expected ELF64 little-endian x86-64 relocatable object, not executable")
                check_integrity(before)
                row["actual_environment_after"] = machine_snapshot()
                require(manifest["environment"] == row["actual_environment_after"], "post-job environment drift")
                row["status"] = "passed"
            except (ValueError, OSError, KeyError, TypeError) as error:
                row["error"] = str(error)
            finally:
                result["jobs"].append(row)
                write_json_new(root / (job["id"] + ".record.json"), row)
                for path in [Path(job[k]) for k in ("report", "object", "stdout", "stderr")] + [root / (job["id"] + ".record.json")]:
                    if path.is_file() and not path.is_symlink():
                        owned[str(path)] = file_identity(path)
            require(row["status"] == "passed", f"profile job blocked: {job['id']}")
        source_qualifiers(manifest, root, "post")
        native_qualifiers(inventory)
        check_integrity(before)
        check_integrity(owned)
        # The manifest fixes only the approved same-semantics role pairs.
        # Job selection binds units, metric, emit-object mode and fixed env.
        comparisons = []
        for metric in ("count", "bytes"):
            for units in (1000, 4000):
                selected = {r["mode"]: r for r in result["jobs"] if r["metric"] == metric and Path(r["source"]).stem == f"corpus-{units}"}
                require(set(selected) == set("TCIU"), "missing/duplicate comparison jobs")
                for before_mode, after_mode in manifest["comparison_pairs"]:
                    a, b = selected[before_mode], selected[after_mode]
                    require(a["phase"] == b["phase"] == "emit-object" and a["generated_profile"] == b["generated_profile"], "incompatible job contexts")
                    comparisons.append({"before_job": a["id"], "after_job": b["id"], "units": units,
                                        "before_source_version": a["source_version"], "after_source_version": b["source_version"],
                                        **compare_reports(a["parsed_report"], b["parsed_report"])})
        result["comparisons"] = comparisons
        result["matched_metric_processes"] = [{"source": entry["source"], "source_sha256": entry["sha256"],
            "jobs": {metric: next(row["id"] for row in result["jobs"] if row["metric"] == metric and row["source"] == str(INPUT_ROOT / entry["source"]))
                     for metric in ("count", "bytes")}} for entry in corpus_entries(inventory)]
        result["status"] = "passed-bounded-compiler-profile-only"
    except (ValueError, OSError, KeyError, TypeError) as error:
        result["error"] = str(error)
    finally:
        try:
            check_integrity(before)
            if "owned" in locals():
                check_integrity(owned)
        except (ValueError, OSError) as error:
            result["status"] = "blocked"
            result["integrity_error"] = str(error)
        write_json_new(root / "result.json", result)
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    plan = sub.add_parser("plan", help="materialize nonexecuting, unauthorized emit-object schedule")
    plan.add_argument("--output", type=Path, required=True)
    run = sub.add_parser("execute", help="future authorized profile window ONLY; no timing/perf execution")
    run.add_argument("--manifest", type=Path, required=True)
    run.add_argument("--manifest-sha256", required=True, help="parent-pinned canonical manifest hash")
    run.add_argument("--output", type=Path, required=True)
    run.add_argument("--allow-profile", action="store_true")
    run.add_argument("--timeout", type=float, default=300, help="safety bound, not a sample")
    args = parser.parse_args(argv)
    try:
        if args.action == "plan":
            inventory, records = accepted_inventory(), accepted_records()
            root = new_root(args.output)
            write_json_new(root / "plan.json", make_plan(inventory, {r: canonical_build(records, r) for r in SOURCE_PINS}, root))
            return 0
        result = execute(args.manifest, args.manifest_sha256, args.output, args.allow_profile, args.timeout)
        return 0 if result["status"] == "passed-bounded-compiler-profile-only" else 1
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(1, f"BLOCKED: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
