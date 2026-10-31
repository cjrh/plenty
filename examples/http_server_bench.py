#!/usr/bin/env python3
"""Compare throughput, latency and memory of Plenty HTTP servers under ApacheBench.

Run from the repository root with a release compiler:

    cargo build --release --locked
    python3 examples/http_server_bench.py --compiler target/release/plenty \
        --output target/http-bench --runs 3

With `--tls` the same workloads run over HTTPS against
examples/https_server_bench.plenty, with the certificate in tests/fixtures/tls
(ECDSA P-256); ApacheBench does not verify it. ApacheBench cannot measure new
HTTPS connections: it counts connections closed without close_notify as
failed. That workload runs
`curl --parallel` instead, in four processes on the client CPUs, with
TCP_NODELAY (curl's default) and without certificate verification.

Select `--implementation actor` for std.actor.http.server, or `--source FILE`
for another fixture with the same routes and `port N` startup line. Actor
fixtures accept CAPACITY; worker fixtures accept WORKERS and IDLE. Both use
CERT/KEY for TLS and answer /, /16k, /echo and /stop like the original fixture.

For an equal-core comparison use `--workers 1 --server-cpus 0` for the worker
server and `--implementation actor --server-cpus 0`; keep client affinity and
all workload settings equal, including `--idle 984` (the actor default; worker
keeps its historical 1000). Actor capacity must accommodate the highest workload
concurrency; this driver measures admitted traffic, not deliberate overload.
Separately compare each server's default setup.
CPU defaults are 0,2,4,6 for the server and 8,10,12,14 for the client (physical
cores on the reference machine); override them for your machine. Each workload
runs `--runs` times against a fresh server; the summary reports the median
and range. Results, with the compiler hash and machine
details, go to OUTPUT/results.json. A measured failure makes the driver exit 1.
The published baseline is in book/src/design/stdlib/http-qualification.md.
"""

import argparse
import csv
import hashlib
import json
import os
import platform
import resource
import select
import socket
import ssl
import statistics
import subprocess
import time
from pathlib import Path

SERVER_CPUS = "0,2,4,6"
CLIENT_CPUS = "8,10,12,14"
ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "examples" / "http_server_bench.plenty"
TLS_SOURCE = ROOT / "examples" / "https_server_bench.plenty"
ACTOR_SOURCE = ROOT / "examples" / "actor_http_server_bench.plenty"
ACTOR_TLS_SOURCE = ROOT / "examples" / "actor_https_server_bench.plenty"
TLS_FIXTURES = ROOT / "tests" / "fixtures" / "tls"
# The actor server's public Config limit, below the runtime's heap-slot limit.
ACTOR_MAX_CONNECTIONS = 1000
IDLE_HEADROOM = 16

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


def tls_env():
    return {"CERT": str(TLS_FIXTURES / "leaf.pem"), "KEY": str(TLS_FIXTURES / "leaf.key")}


def connect(port, tls):
    conn = socket.create_connection(("127.0.0.1", port), timeout=30)
    if not tls:
        return conn
    context = ssl.create_default_context(cafile=TLS_FIXTURES / "ca.pem")
    return context.wrap_socket(conn, server_hostname="localhost")


class Server:
    def __init__(self, executable, env, tls, cpus=SERVER_CPUS):
        self.tls = tls
        if tls:
            env = {**env, **tls_env()}
        self.process = subprocess.Popen(
            ["taskset", "-c", cpus, str(executable)],
            stdout=subprocess.PIPE,
            env={**os.environ, **env},
            preexec_fn=raise_descriptor_limit,
            text=True,
        )
        try:
            if not select.select([self.process.stdout], [], [], 30)[0]:
                raise RuntimeError("server did not announce its port within 30 seconds")
            line = self.process.stdout.readline()
            words = line.split()
            if len(words) != 2 or words[0] != "port":
                raise RuntimeError(f"invalid server startup line: {line!r}")
            self.port = int(words[1])
        except BaseException:
            self.close()
            raise

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def close(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.communicate()

    def get(self, path):
        with connect(self.port, self.tls) as conn:
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


def run_ab(port, tls, path, keep_alive, concurrency, requests, body, scratch, cpus=CLIENT_CPUS):
    percentiles = scratch / "percentiles.csv"
    command = ["taskset", "-c", cpus, "ab", "-q", "-r", "-n", str(requests),
               "-c", str(concurrency), "-e", str(percentiles)]
    if keep_alive:
        command.append("-k")
    if body:
        payload = scratch / "body"
        payload.write_bytes(b"p" * body)
        command += ["-p", str(payload), "-T", "application/octet-stream"]
    scheme = "https" if tls else "http"
    command.append(f"{scheme}://127.0.0.1:{port}{path}")
    out = subprocess.run(command, capture_output=True, text=True, check=True).stdout
    result = parse_ab(out, requests)
    with percentiles.open() as handle:
        rows = {int(float(row[0])): float(row[1]) for row in csv.reader(handle) if row and row[0][0].isdigit()}
    result["p50_ms"] = rows[50]
    result["p99_ms"] = rows[99]
    return result


def parse_ab(out, requests):
    result = {"non_2xx": 0}
    for line in out.splitlines():
        if line.startswith("Requests per second:"):
            result["rps"] = float(line.split()[3])
        elif line.startswith("Complete requests:"):
            result["completed"] = int(line.split()[2])
        elif line.startswith("Failed requests:"):
            result["failed"] = int(line.split()[2])
        elif line.startswith("Non-2xx responses:"):
            result["non_2xx"] = int(line.split()[2])
    if not {"rps", "completed", "failed"} <= result.keys():
        raise ValueError(f"incomplete ApacheBench report: {out}")
    if result["completed"] > requests:
        raise ValueError("ApacheBench reported more requests than requested")
    result["missing"] = requests - result["completed"]
    result["failed"] += result["missing"]
    return result


def run_curl(port, path, concurrency, requests, scratch, cpus=CLIENT_CPUS):
    """New HTTPS connection per request: four curl processes in parallel."""
    processes = min(4, concurrency, requests)
    counts = [requests // processes + (index < requests % processes) for index in range(processes)]
    start = time.perf_counter()
    running = []
    for index, count in enumerate(counts):
        url = f"https://127.0.0.1:{port}{path}?[1-{count}]"
        out = (scratch / f"curl-{index}.txt").open("w")
        parallel = concurrency // processes + (index < concurrency % processes)
        command = ["taskset", "-c", cpus, "curl", "-s", "-k", "-Z", "--parallel-max",
                   str(parallel), "--max-time", "30", "-H", "Connection: close", "-o", "/dev/null",
                   "-w", "%{http_code} %{time_total}\n", url]
        running.append((subprocess.Popen(command, stdout=out), out))
    try:
        for process, out in running:
            process.wait(timeout=120)
    finally:
        for process, out in running:
            if process.poll() is None:
                process.kill()
                process.wait()
            out.close()
    elapsed = time.perf_counter() - start
    lines = [line for index in range(processes)
             for line in (scratch / f"curl-{index}.txt").read_text().splitlines()]
    result = parse_curl(lines, requests, elapsed)
    result["client_exit_codes"] = [process.returncode for process, _ in running]
    # An unexplained client failure must never become a clean benchmark.
    if any(result["client_exit_codes"]) and result["failed"] == 0:
        result["failed"] = 1
    return result


def parse_curl(lines, requests, elapsed):
    times, failed = [], 0
    for line in lines:
        code, seconds = line.split()
        failed += code != "200"
        times.append(float(seconds) * 1000)
    if not times or len(times) > requests:
        raise ValueError("curl returned an invalid number of request results")
    times.sort()
    missing = requests - len(times)
    return {"rps": round(len(times) / elapsed, 2), "failed": failed + missing,
            "completed": len(times), "missing": missing, "non_2xx": 0,
            "p50_ms": round(times[len(times) // 2], 3),
            "p99_ms": round(times[len(times) * 99 // 100], 3)}


def idle_memory(executable, count, tls, env=None, cpus=SERVER_CPUS):
    """Resident memory added by `count` parked keep-alive connections."""
    settings = {**(env or {}), "IDLE": str(count + IDLE_HEADROOM), "CAPACITY": str(count + IDLE_HEADROOM)}
    with Server(executable, settings, tls, cpus) as server:
        time.sleep(0.2)
        before = int(status_fields(server.process.pid)["VmRSS"].split()[0])
        conns = []
        try:
            for _ in range(count):
                conn = connect(server.port, tls)
                conns.append(conn)
                conn.sendall(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            for conn in conns:
                data = b""
                while not data.endswith(b"hello, world\n"):
                    chunk = conn.recv(4096)
                    if not chunk:
                        raise RuntimeError("idle connection closed before its response completed")
                    data += chunk
            time.sleep(0.2)
            after = int(status_fields(server.process.pid)["VmRSS"].split()[0])
        finally:
            for conn in conns:
                conn.close()
        server.stop()
    return {"connections": count, "settings": settings,
            "rss_before_kib": before, "rss_after_kib": after,
            "kib_per_connection": round((after - before) / count, 2)}


def describe(compiler, tls, source, server_cpus, client_cpus):
    def text(command):
        return subprocess.run(command, capture_output=True, text=True).stdout.strip()
    cpu = next(line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
               if line.startswith("model name"))
    return {
        "compiler_sha256": hashlib.sha256(Path(compiler).read_bytes()).hexdigest(),
        "compiler_target": text([compiler, "--print-target"]),
        "compiler_version": text([compiler, "--version"]),
        "revision": text(["git", "-C", str(ROOT), "rev-parse", "HEAD"]),
        "dirty": bool(text(["git", "-C", str(ROOT), "status", "--porcelain", "--untracked-files=no"])),
        "source": str(source),
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "kernel": platform.release(),
        "cpu": cpu,
        "ab": text(["ab", "-V"]).splitlines()[0],
        "curl": text(["curl", "--version"]).splitlines()[0] if tls else None,
        "tls": tls,
        "openssl": text(["openssl", "version"]) if tls else None,
        "load_before": os.getloadavg(),
        "server_cpus": server_cpus,
        "client_cpus": client_cpus,
    }


def positive(value):
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--compiler", required=True)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--runs", type=positive, default=3)
    parser.add_argument("--workers", type=positive, default=4)
    parser.add_argument("--idle", type=positive, help="idle connections to measure (worker: 1000, actor: 984)")
    parser.add_argument("--tls", action="store_true", help="serve HTTPS")
    parser.add_argument("--implementation", choices=("worker", "actor"), default="worker")
    parser.add_argument("--source", type=Path, help="override the benchmark server fixture")
    parser.add_argument("--capacity", type=positive, default=256, help="actor connection capacity during load tests")
    parser.add_argument("--server-cpus", default=SERVER_CPUS)
    parser.add_argument("--client-cpus", default=CLIENT_CPUS)
    parser.add_argument("--requests", type=positive, help="requests per workload (default: workload-specific)")
    args = parser.parse_args(argv)
    if args.idle is None:
        args.idle = ACTOR_MAX_CONNECTIONS - IDLE_HEADROOM if args.implementation == "actor" else 1000
    if args.implementation == "actor":
        if args.capacity > ACTOR_MAX_CONNECTIONS or args.idle + IDLE_HEADROOM > ACTOR_MAX_CONNECTIONS:
            parser.error("actor capacity must be at most 1000 and idle connections at most 984")
        concurrency = max(min(workload[3], args.requests or workload[4]) for workload in WORKLOADS)
        if args.capacity < concurrency:
            parser.error(f"actor capacity must be at least the highest workload concurrency ({concurrency})")
    return args


def main():
    args = arguments()
    args.output.mkdir(parents=True, exist_ok=True)
    executable = args.output / "server"
    source = args.source or ((ACTOR_TLS_SOURCE if args.tls else ACTOR_SOURCE)
                             if args.implementation == "actor" else (TLS_SOURCE if args.tls else SOURCE))
    source = source.resolve()
    subprocess.run([args.compiler, "--release", "--compile", str(source), "-o", str(executable)], check=True)
    env = ({"WORKERS": str(args.workers), "IDLE": "256"} if args.implementation == "worker"
           else {"CAPACITY": str(args.capacity)})
    results = {"machine": describe(args.compiler, args.tls, source, args.server_cpus, args.client_cpus),
               "implementation": args.implementation, "settings": env,
               "workers": args.workers if args.implementation == "worker" else None, "workloads": []}
    for name, path, keep_alive, concurrency, requests, body in WORKLOADS:
        requests = args.requests or requests
        concurrency = min(concurrency, requests)
        runs = []
        for _ in range(args.runs):
            with Server(executable, env, args.tls, args.server_cpus) as server:
                if args.tls and not keep_alive:
                    measured = run_curl(server.port, path, concurrency, requests, args.output, args.client_cpus)
                else:
                    measured = run_ab(server.port, args.tls, path, keep_alive, concurrency, requests, body, args.output, args.client_cpus)
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
    results["idle"] = idle_memory(executable, args.idle, args.tls, env, args.server_cpus)
    print(f"idle: {results['idle']}")
    (args.output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    return int(any(item["failed"] for item in results["workloads"]))


if __name__ == "__main__":
    raise SystemExit(main())
