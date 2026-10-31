"""Benchmark client failures must not become apparently clean comparisons."""
import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("http_server_bench", ROOT / "examples/http_server_bench.py")
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)


class BenchmarkTests(unittest.TestCase):
    def test_ab_counts_missing_failed_and_non_success_responses(self):
        result = bench.parse_ab("""Complete requests: 8
Failed requests: 1
Non-2xx responses: 2
Requests per second: 400.00 [#/sec] (mean)
""", 10)
        self.assertEqual(result["completed"], 8)
        self.assertEqual(result["missing"], 2)
        self.assertEqual(result["failed"], 3)
        self.assertEqual(result["non_2xx"], 2)
        with self.assertRaises(ValueError):
            bench.parse_ab("Requests per second: 400.00", 10)

    def test_curl_counts_failed_statuses_and_missing_results(self):
        result = bench.parse_curl(["200 0.001", "503 0.002", "000 0.030"], 5, 0.1)
        self.assertEqual(result["failed"], 4)
        self.assertEqual(result["missing"], 2)
        self.assertEqual(result["rps"], 30)
        with self.assertRaises(ValueError):
            bench.parse_curl([], 5, 0.1)

    def test_curl_sends_every_request_even_when_not_divisible_by_four(self):
        commands = []

        def launch(command, stdout):
            commands.append(command)
            count = int(command[-1].rsplit("-", 1)[1][:-1])
            stdout.write("200 0.001\n" * count)
            process = MagicMock()
            process.returncode = 0
            process.poll.return_value = 0
            return process

        with tempfile.TemporaryDirectory() as directory:
            with patch.object(bench.subprocess, "Popen", side_effect=launch):
                result = bench.run_curl(8000, "/", 3, 5, Path(directory), "1,2")
        self.assertEqual(result["completed"], 5)
        self.assertEqual(result["failed"], 0)
        self.assertEqual(len(commands), 3)
        self.assertTrue(all(command[:3] == ["taskset", "-c", "1,2"] for command in commands))
        self.assertEqual(sum(int(c[c.index("--parallel-max") + 1]) for c in commands), 3)

    def test_idle_measurement_rejects_early_eof_and_closes_resources(self):
        server = MagicMock()
        server.__enter__.return_value = server
        connection = MagicMock()
        connection.recv.return_value = b""
        with patch.object(bench, "Server", return_value=server), \
             patch.object(bench, "connect", return_value=connection), \
             patch.object(bench, "status_fields", return_value={"VmRSS": "100 KiB"}), \
             patch.object(bench.time, "sleep"):
            with self.assertRaisesRegex(RuntimeError, "closed before"):
                bench.idle_memory(Path("server"), 1, False)
        connection.close.assert_called_once()
        server.__exit__.assert_called_once()

    def test_existing_cli_and_actor_capacity_validation(self):
        base = ["--compiler", "plenty", "--output", "/tmp/results"]
        args = bench.arguments(base)
        self.assertEqual((args.implementation, args.workers, args.idle), ("worker", 4, 1000))
        args = bench.arguments(base + ["--implementation", "actor", "--server-cpus", "0", "--requests", "2"])
        self.assertEqual((args.server_cpus, args.requests, args.idle), ("0", 2, 984))
        args = bench.arguments(base + ["--implementation", "actor", "--capacity", "2", "--requests", "2"])
        self.assertEqual(args.capacity, 2)
        with contextlib.redirect_stderr(io.StringIO()):
            for invalid in (["--runs", "0"], ["--implementation", "actor", "--capacity", "1001"],
                            ["--implementation", "actor", "--idle", "985"],
                            ["--implementation", "actor", "--capacity", "63"]):
                with self.assertRaises(SystemExit):
                    bench.arguments(base + invalid)

    def test_failed_startup_reaps_server(self):
        process = MagicMock()
        process.stdout.readline.return_value = "not a port\n"
        process.poll.return_value = None
        with patch.object(bench.subprocess, "Popen", return_value=process), \
             patch.object(bench.select, "select", return_value=([process.stdout], [], [])):
            with self.assertRaisesRegex(RuntimeError, "startup"):
                bench.Server(Path("server"), {}, False)
        process.kill.assert_called_once()
        process.communicate.assert_called_once()


if __name__ == "__main__":
    unittest.main()
