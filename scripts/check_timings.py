#!/usr/bin/env python3
"""Coarse wall-clock reports for local checks and GitHub Actions; stdlib only."""
import argparse
from collections import defaultdict
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parent.parent


def stamp():
    return datetime.now(timezone.utc).isoformat()


def git_revision():
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()


def local_report(label, rows, elapsed):
    return {"schema": 1, "kind": "local", "label": label, "created_at": stamp(),
            "revision": git_revision(), "host": platform.node(), "platform": f"{sys.platform} {platform.machine()}",
            "elapsed_seconds": elapsed, "rows": rows}


def cell(value):
    return str(value).replace("|", "\\|").replace("\n", " ").replace("\r", " ")


def render(data, baseline=None):
    if baseline and baseline["kind"] != data["kind"]:
        raise ValueError("cannot compare local checks with CI runner timings")
    rows = data["rows"]
    total = sum(row["seconds"] for row in rows)
    lines = [f'# Check timings: {cell(data["label"])}', "",
             f'Revision: `{data["revision"]}`. Collected: {data["created_at"]}.', "",
             f'Elapsed span: **{data["elapsed_seconds"]:.1f}s**. '
             f'Sum of measured {"steps" if data["kind"] == "local" else "runner steps"}: **{total:.1f}s**.', ""]
    if data["kind"] == "ci":
        lines += ["Jobs run in parallel; summed runner time is not workflow latency. "
                  "The span runs from the first job start to the last job finish; initial queue time is excluded. "
                  "Unfinished jobs have no duration yet; unfinished steps are excluded. The reporting job is excluded.", "",
                  "| Job | Status | Seconds |", "| --- | --- | ---: |"]
        for job in sorted(data["jobs"], key=lambda j: j["seconds"] or 0, reverse=True):
            seconds = "pending" if job["seconds"] is None else f'{job["seconds"]:.1f}'
            lines.append(f'| {cell(job["name"])} | {cell(job["status"])} | {seconds} |')
        lines += ["", "Step names below are aggregated across jobs, architectures and shards. "
                  "Counts expose matrix changes; compare the same workflow, attempt scope and cache conditions.", ""]
    else:
        lines += [f'Host: {cell(data["host"])} ({cell(data["platform"])}). '
                  "Includes command startup, Cargo lock waits and cache effects; no CPU profiling.", ""]
    grouped = defaultdict(lambda: [0.0, 0, set()])
    old = defaultdict(lambda: [0.0, 0])
    for row in rows:
        value = grouped[row["name"]]
        value[0] += row["seconds"]
        value[1] += 1
        value[2].add(row["status"])
    if baseline:
        for row in baseline["rows"]:
            old[row["name"]][0] += row["seconds"]
            old[row["name"]][1] += 1
        lines += [f'Baseline: {cell(baseline["label"])} (`{baseline["revision"]}`). '
                  "Deltas are current minus baseline; changed counts are marked instead of compared.", ""]
    lines += ["| Step | Count | Status | Seconds | Share |" + (" Delta |" if baseline else ""),
              "| --- | ---: | --- | ---: | ---: |" + (" ---: |" if baseline else "")]
    for name in sorted(set(grouped) | set(old), key=lambda n: grouped[n][0], reverse=True):
        seconds, count, statuses = grouped[name]
        share = seconds / total * 100 if total else 0
        line = f'| {cell(name)} | {count} | {cell(", ".join(sorted(statuses)))} | {seconds:.1f} | {share:.1f}% |'
        if baseline:
            previous, previous_count = old[name]
            delta = f'{seconds - previous:+.1f}s' if count == previous_count else f'count {previous_count} → {count}'
            line += f' {delta} |'
        lines.append(line)
    return "\n".join(lines) + "\n"


def save(data, output, baseline=None):
    output.mkdir(parents=True, exist_ok=True)
    report = render(data, baseline)
    (output / "timings.json").write_text(json.dumps(data, indent=2) + "\n")
    (output / "timings.md").write_text(report)
    print(report)
    print(f'Report: {output / "timings.md"}', file=sys.stderr)


def output_directory(kind):
    return REPO / "target/check-timings" / f'{kind}-{datetime.now(timezone.utc):%Y%m%dT%H%M%S}-{os.getpid()}'


def run_suite(args):
    output = args.output or output_directory("suite")
    output.mkdir(parents=True, exist_ok=False)
    base = ["cargo", "test", "--locked", "--workspace", "--all-features"]
    if args.profile != "dev":
        base += ["--profile", args.profile]
    started = time.monotonic()
    rows = []
    result = 0
    for name, command in [("Compile tests", base + ["--no-run", "--timings"]),
                          ("Run tests and doctests", base)]:
        print(f'== {name}: {" ".join(command)}', flush=True)
        before = time.monotonic()
        try:
            result = subprocess.run(command, cwd=REPO).returncode
        except OSError as error:
            print(error, file=sys.stderr)
            result = 127
        except KeyboardInterrupt:
            result = 130
        rows.append({"name": name, "seconds": time.monotonic() - before,
                     "status": "ok" if result == 0 else f"failed ({result})", "command": command})
        save(local_report(f"Full suite ({args.profile})", rows, time.monotonic() - started), output)
        if result:
            break
    return result if result >= 0 else 128 - result


def duration(item):
    start, end = item.get("started_at"), item.get("completed_at")
    if not start or not end:
        return None
    seconds = (datetime.fromisoformat(end.replace("Z", "+00:00")) -
               datetime.fromisoformat(start.replace("Z", "+00:00"))).total_seconds()
    # Actions uses year-0001 timestamps for steps that never ran.
    return max(0.0, seconds) if not start.startswith("0001-") else None


def ci_report(pages, label, revision):
    jobs = [job for page in pages for job in page["jobs"] if job["name"] != "Check timing report"]
    rows, summaries, starts, ends = [], [], [], []
    for job in jobs:
        seconds = duration(job)
        summaries.append({"name": job["name"], "seconds": seconds,
                          "status": job.get("conclusion") or job["status"]})
        if seconds is not None:
            starts.append(job["started_at"])
            ends.append(job["completed_at"])
        for step in job.get("steps", []):
            seconds = duration(step)
            if seconds is not None:
                rows.append({"name": step["name"], "job": job["name"], "seconds": seconds,
                             "status": step.get("conclusion") or step["status"]})
    span = duration({"started_at": min(starts), "completed_at": max(ends)}) if starts else 0
    return {"schema": 1, "kind": "ci", "label": label, "revision": revision,
            "created_at": stamp(), "elapsed_seconds": span, "jobs": summaries, "rows": rows}


def gh_json(endpoint, paginate=False):
    command = ["gh", "api", endpoint]
    if paginate:
        command += ["--paginate", "--slurp"]
    env = {**os.environ, "GH_PROMPT_DISABLED": "1", "GH_PAGER": "cat", "GH_NO_UPDATE_NOTIFIER": "1"}
    return json.loads(subprocess.check_output(command, env=env, text=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    run = sub.add_parser("suite", help="run and time the full all-features Cargo suite")
    run.add_argument("--profile", choices=["dev", "ci"], default="dev")
    run.add_argument("--output", type=Path)
    local = sub.add_parser("local", help="convert local_ci.sh's TSV into a saved report")
    local.add_argument("input", type=Path)
    local.add_argument("--elapsed", type=float, required=True)
    local.add_argument("--output", type=Path, required=True)
    ci = sub.add_parser("ci", help="read Actions job/step timestamps, without downloading logs")
    ci.add_argument("--repo", required=True, help="OWNER/REPO")
    ci.add_argument("--run", required=True, type=int)
    ci.add_argument("--output", type=Path)
    report = sub.add_parser("report", help="render a saved report, optionally against a baseline")
    report.add_argument("input", type=Path)
    report.add_argument("--baseline", type=Path)
    args = parser.parse_args()
    if args.mode == "suite":
        return run_suite(args)
    if args.mode == "local":
        rows = []
        for line in args.input.read_text().splitlines():
            name, status, seconds = line.split("\t")
            rows.append({"name": name, "status": status, "seconds": float(seconds)})
        save(local_report("Local CI", rows, args.elapsed), args.output)
    elif args.mode == "ci":
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo) or args.run <= 0:
            parser.error("expected --repo OWNER/REPO and a positive --run ID")
        endpoint = f"repos/{args.repo}/actions/runs/{args.run}"
        meta = gh_json(endpoint)
        attempt = meta["run_attempt"]
        pages = gh_json(f"{endpoint}/attempts/{attempt}/jobs?per_page=100", paginate=True)
        output = args.output or output_directory(f"ci-{args.run}-{attempt}")
        output.mkdir(parents=True, exist_ok=False)
        (output / "run.json").write_text(json.dumps(meta, indent=2) + "\n")
        (output / "jobs.json").write_text(json.dumps(pages, indent=2) + "\n")
        save(ci_report(pages, f'{args.repo} run {args.run}, attempt {attempt}', meta["head_sha"]), output)
    else:
        data = json.loads(args.input.read_text())
        baseline = json.loads(args.baseline.read_text()) if args.baseline else None
        print(render(data, baseline), end="")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"check_timings: {error}")
