#!/usr/bin/env python3
"""Throughput, latency and memory of std.http.server under ApacheBench.

Run from the repository root with a release compiler:

    cargo build --release --locked
    python3 examples/http_server_bench.py --compiler target/release/plenty \
        --output target/http-bench --runs 3

The server runs on CPUs 0,2,4,6 and ab on 8,10,12,14 (one hardware thread
per physical core on the reference machine), so no more than ten threads are
busy. Each workload runs `--runs` times against a fresh server; the summary
reports the median and range. Results, with the compiler hash and machine
details, go to OUTPUT/results.json. The published baseline is in
book/src/design/stdlib/http-qualification.md.
"""

import argparse
import csv
import hashlib
import json
import os
import platform
import resource
import socket
import statistics
import subprocess
import time
from pathlib import Path

SERVER_CPUS = "0,2,4,6"
CLIENT_CPUS = "8,10,12,14"
SOURCE = Path(__file__).with_name("http_server_bench.plenty")

# name, path, keep-alive, concurrency, requests, POST body size
WORKLOADS = [
    ("keep-alive, 1 client", "/", True, 1, 20000, 0),
    ("keep-alive, 16 clients", "/", True, 16, 100000, 0),
    ("keep-alive, 64 clients", "/", True, 64, 100000, 0),
    ("new connection per request, 16 clients", "/", False, 16, 20000, 0),
    ("keep-alive 16 KiB response, 16 clients", "/16k", True, 16, 50000, 0),
    ("keep-alive 1 KiB POST echo, 16 clients", "/echo", True, 16, 50000, 1024),
]


def raise_descriptor_limit():
    soft, hard = resource.getrlimit(resource.RLIMIT_NOFILE)
    resource.setrlimit(resource.RLIMIT_NOFILE, (hard, hard))


def status_fields(pid):
    fields = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        key, _, value = line.partition(":")
        fields[key] = value.strip()
    return fields


def cpu_seconds(pid):
    stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    ticks = os.sysconf("SC_CLK_TCK")
    return (int(stat[11]) + int(stat[12])) / ticks


class Server:
    def __init__(self, executable, env):
        self.process = subprocess.Popen(
            ["taskset", "-c", SERVER_CPUS, str(executable)],
            stdout=subprocess.PIPE,
            env={**os.environ, **env},
            preexec_fn=raise_descriptor_limit,
            text=True,
        )
        line = self.process.stdout.readline()
        self.port = int(line.split()[1])

    def get(self, path):
        with socket.create_connection(("127.0.0.1", self.port)) as conn:
            conn.sendall(f"GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n".encode())
            data = b""
            while chunk := conn.recv(65536):
                data += chunk
        return data

    def stop(self):
        self.get("/stop")
        out, _ = self.process.communicate(timeout=30)
        assert self.process.returncode == 0, out
        return out.strip()


def run_ab(port, path, keep_alive, concurrency, requests, body, scratch):
    percentiles = scratch / "percentiles.csv"
    command = ["taskset", "-c", CLIENT_CPUS, "ab", "-q", "-r", "-n", str(requests),
               "-c", str(concurrency), "-e", str(percentiles)]
    if keep_alive:
        command.append("-k")
    if body:
        payload = scratch / "body"
        payload.write_bytes(b"p" * body)
        command += ["-p", str(payload), "-T", "application/octet-stream"]
    command.append(f"http://127.0.0.1:{port}{path}")
    out = subprocess.run(command, capture_output=True, text=True, check=True).stdout
    result = {}
    for line in out.splitlines():
        if line.startswith("Requests per second:"):
            result["rps"] = float(line.split()[3])
        elif line.startswith("Failed requests:"):
            result["failed"] = int(line.split()[2])
        elif line.startswith("Non-2xx responses:"):
            result["non_2xx"] = int(line.split()[2])
    with percentiles.open() as handle:
        rows = {int(float(row[0])): float(row[1]) for row in csv.reader(handle) if row[0][0].isdigit()}
    result["p50_ms"] = rows[50]
    result["p99_ms"] = rows[99]
    result.setdefault("non_2xx", 0)
    return result


def idle_memory(executable, count):
    """Resident memory added by `count` parked keep-alive connections."""
    server = Server(executable, {"IDLE": str(count + 16)})
    time.sleep(0.2)
    before = int(status_fields(server.process.pid)["VmRSS"].split()[0])
    conns = []
    for _ in range(count):
        conn = socket.create_connection(("127.0.0.1", server.port))
        conn.sendall(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        conns.append(conn)
    for conn in conns:
        data = b""
        while not data.endswith(b"hello, world\n"):
            data += conn.recv(4096)
    time.sleep(0.2)
    after = int(status_fields(server.process.pid)["VmRSS"].split()[0])
    for conn in conns:
        conn.close()
    server.stop()
    return {"connections": count, "rss_before_kib": before, "rss_after_kib": after,
            "kib_per_connection": round((after - before) / count, 2)}


def describe(compiler):
    def text(command):
        return subprocess.run(command, capture_output=True, text=True).stdout.strip()
    cpu = next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
               if line.startswith("model name"))
    return {
        "compiler_sha256": hashlib.sha256(Path(compiler).read_bytes()).hexdigest(),
        "compiler_target": text([compiler, "--print-target"]),
        "revision": text(["git", "rev-parse", "--short", "HEAD"]),
        "kernel": platform.release(),
        "cpu": cpu,
        "ab": text(["ab", "-V"]).splitlines()[0],
        "load_before": os.getloadavg(),
        "server_cpus": SERVER_CPUS,
        "client_cpus": CLIENT_CPUS,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--idle", type=int, default=1000)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    executable = args.output / "server"
    subprocess.run([args.compiler, "--release", "--compile", str(SOURCE), "-o", str(executable)], check=True)
    results = {"machine": describe(args.compiler), "workers": args.workers, "workloads": []}
    for name, path, keep_alive, concurrency, requests, body in WORKLOADS:
        runs = []
        for _ in range(args.runs):
            server = Server(executable, {"WORKERS": str(args.workers)})
            measured = run_ab(server.port, path, keep_alive, concurrency, requests, body, args.output)
            fields = status_fields(server.process.pid)
            measured["peak_rss_kib"] = int(fields["VmHWM"].split()[0])
            measured["threads"] = int(fields["Threads"])
            measured["cpu_seconds"] = cpu_seconds(server.process.pid)
            measured["report"] = server.stop()
            runs.append(measured)
        rps = [run["rps"] for run in runs]
        summary = {
            "name": name, "path": path, "keep_alive": keep_alive, "concurrency": concurrency,
            "requests": requests, "body": body, "runs": runs,
            "rps_median": statistics.median(rps), "rps_min": min(rps), "rps_max": max(rps),
            "p50_ms_median": statistics.median(run["p50_ms"] for run in runs),
            "p99_ms_median": statistics.median(run["p99_ms"] for run in runs),
            "peak_rss_kib_max": max(run["peak_rss_kib"] for run in runs),
            "failed": sum(run["failed"] + run["non_2xx"] for run in runs),
        }
        results["workloads"].append(summary)
        print(f"{name}: {summary['rps_median']:.0f} req/s ({summary['rps_min']:.0f}-{summary['rps_max']:.0f}), "
              f"p50 {summary['p50_ms_median']} ms, p99 {summary['p99_ms_median']} ms, "
              f"peak RSS {summary['peak_rss_kib_max']} KiB, failed {summary['failed']}", flush=True)
    results["idle"] = idle_memory(executable, args.idle)
    print(f"idle: {results['idle']}")
    (args.output / "results.json").write_text(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
