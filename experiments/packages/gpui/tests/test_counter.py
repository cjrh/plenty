#!/usr/bin/env python3
"""End-to-end Plenty ABI tests, not a GPUI renderer test. Run from any cwd."""
import argparse
from pathlib import Path
import subprocess
import tempfile

PACKAGE = Path(__file__).resolve().parents[1]


def run(command, *, success=True):
    result = subprocess.run(command, text=True, capture_output=True, cwd=PACKAGE, timeout=600)
    if (result.returncode == 0) != success:
        raise AssertionError(f"{command}: {result.returncode}\n{result.stdout}\n{result.stderr}")
    return result


def build(compiler, source, fast=False):
    command = ["python3", "-B", str(PACKAGE / "build.py"), "--plenty", str(compiler), str(source)]
    if fast:
        command.append("--fast-compile")
    run(command)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--smoke", action="store_true", help="also verify original destruction after real X11 close")
    parser.add_argument("--plenty", type=Path, default=PACKAGE.parents[2] / "target/release/plenty")
    args = parser.parse_args()
    compiler = args.plenty.resolve(strict=True)
    runner = str(PACKAGE / "native/target/debug/plenty-gpui")
    counter = PACKAGE / "examples/counter.plenty"
    with tempfile.TemporaryDirectory(prefix="plenty-gpui-tests-") as directory:
        root = Path(directory)
        source = counter.read_text()
        # Instrument the originating Plenty destructor, not a Rust-side proxy.
        source = source.replace("    count: i64\n", '    count: i64\n    def __del__(self) -> ():\n        drop(print("STATE_DESTROYED"))\n')
        app = root / "application.plenty"
        app.write_text(source)
        for fast in [False, True]:
            build(compiler, app, fast)
            result = run([runner, "--headless", "1", "1", "2"])
            assert [line for line in result.stdout.splitlines() if line.startswith("Count:")] == [
                "Count: 0", "Count: 1", "Count: 2", "Count: 0"]
            assert result.stdout.count("STATE_DESTROYED") == 1
            result = run([runner, "--headless", "999"], success=False)
            assert "unknown action ID: 999" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # An event error may leave mutations, but must still stop and release.
            app.write_text(source.replace("case 1u64:", "case 999u64:"))
            build(compiler, app, fast)
            result = run([runner, "--headless", "1"], success=False)
            assert "Plenty event failed: InvalidArgument" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # Frame errors prevent events and release once, even if ignored by app.
            app.write_text(source.replace("    frame.end()?\n    Ok(())", "    drop(frame.end())\n    drop(frame.end())\n    Ok(())"))
            build(compiler, app, fast)
            result = run([runner, "--headless"], success=False)
            assert "Plenty render failed: InvalidLayout" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # A malformed render callback configuration never invokes null code.
            app.write_text(source.replace("gpui.Frame(token, begin_function,", "gpui.Frame(token, gpui.Function.null(),"))
            build(compiler, app, fast)
            result = run([runner, "--headless"], success=False)
            assert "Plenty render failed: InvalidContext" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # A failed factory transfers no owner and has nothing to destroy.
            app.write_text(source.replace("Ok(State())", "Err(AllocError.CapacityOverflow)"))
            build(compiler, app, fast)
            result = run([runner, "--headless"], success=False)
            assert "Plenty create failed: AllocError code 1" in result.stderr
            assert "STATE_DESTROYED" not in result.stdout
            app.write_text(source)
        if args.smoke:
            build(compiler, app)
            run(["python3", "-B", str(PACKAGE / "tests/smoke_x11.py"), "--out", "/tmp/plenty-gpui-lifecycle-smoke"])
            assert Path("/tmp/plenty-gpui-lifecycle-smoke/runner.log").read_text().count("STATE_DESTROYED") == 1
        # Verify actual package resolution (as distinct from the launcher's
        # intentionally single-file staging workflow).
        project = root / "consumer"
        (project / "src").mkdir(parents=True)
        (project / "plenty.toml").write_text(
            'manifest-version = 1\n[package]\nname = "consumer"\nversion = "0.0.1"\n'
            f'[dependencies]\ngpui = {{ path = "{PACKAGE}" }}\n')
        (project / "src/main.plenty").write_text('import gpui\ndef main() -> ():\n    drop(gpui.Frame(0u64, gpui.Function.null(), gpui.Function.null(), gpui.Function.null()))\n')
        result = subprocess.run([str(compiler), "check"], cwd=project, capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
        # Unannotated bridge calls are not certified for Plenty worker entry.
        (root / "gpui.plenty").write_text((PACKAGE / "src/lib.plenty").read_text())
        (root / "native.plentyi").write_text((PACKAGE / "src/native.plentyi").read_text())
        (root / "worker.plenty").write_text('import gpui\n@thread_safe\nexport def entry(token: u64, function: gpui.Function) -> Result[(), gpui.GpuiError] = "worker_entry":\n    gpui.Frame(token, function, function, function)?.end()\n')
        result = run([str(compiler), "--check-module", str(root / "worker.plenty")], success=False)
        assert "thread" in result.stderr.lower(), result.stderr
    # Leave the useful uninstrumented counter selected for subsequent smoke tests.
    build(compiler, counter)
    print("PASS: optimized/fast-compile state, events, errors, originating destructor, package, worker rejection")


if __name__ == "__main__":
    main()
