#!/usr/bin/env python3
"""End-to-end Plenty ABI tests, not an iced renderer test. Run from any cwd."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

PACKAGE = Path(__file__).resolve().parents[1]


def run(command, *, success=True, env=None):
    result = subprocess.run(command, text=True, capture_output=True, cwd=PACKAGE, env=env, timeout=600)
    if (result.returncode == 0) != success:
        raise AssertionError(f"{command}: {result.returncode}\n{result.stdout}\n{result.stderr}")
    return result


def build(compiler, source, fast=False, *, success=True):
    command = ["python3", "-B", str(PACKAGE / "build.py"), "--plenty", str(compiler), str(source)]
    if fast:
        command.append("--fast-compile")
    return run(command, success=success)


def check_contract_rejection(compiler, app, source, fast):
    # Both variants are valid Plenty. Only build/link them; never execute an
    # application whose callback ownership differs from the native owner's.
    start = source.index("export def event(")
    end = source.index("def main()", start)
    # Make the event body valid for a by-value (immutable) state parameter.
    event_signature = source[start:source.index("\n", start)].replace("&mut State", "State")
    consuming_event = source[:start] + event_signature + "\n    Ok(())\n\n" + source[end:]
    extra_export = source + '\nexport def extra() -> u32 = "iced_app_extra":\n    0u32\n'
    profile = "fast-compile" if fast else "optimized"
    header = PACKAGE / "target" / profile / "iced_app.h"
    good_header = header.read_text().splitlines()
    [event_prototype] = [line for line in good_header if line.startswith("uint32_t iced_app_event(")]
    guard = (PACKAGE / "native/expected-contract.txt").read_text().strip()
    assert f"void {guard}(void);" in good_header
    for name, variant in [("consuming event", consuming_event), ("extra export", extra_export)]:
        app.write_text(variant)
        result = build(compiler, app, fast, success=False)
        candidate = header.read_text()
        assert event_prototype in candidate.splitlines() and guard not in candidate
        missing = [line for line in result.stderr.splitlines() if guard in line and ("undefined" in line or "not defined" in line)]
        assert missing, result.stderr
        print(f"PASS {profile}: {name}, same C event prototype: {missing[0]}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--smoke", action="store_true", help="also verify original destruction after real X11 close")
    parser.add_argument("--plenty", type=Path, default=PACKAGE.parents[2] / "target/release/plenty")
    args = parser.parse_args()
    compiler = args.plenty.resolve(strict=True)
    counter = PACKAGE / "examples/counter.plenty"
    with tempfile.TemporaryDirectory(prefix="plenty-iced-tests-") as directory:
        root = Path(directory)
        source = counter.read_text()
        # Instrument the originating Plenty destructor, not a Rust-side proxy.
        source = source.replace("    count: i64\n", '    count: i64\n    def __del__(self) -> ():\n        drop(print("STATE_DESTROYED"))\n')
        app = root / "application.plenty"
        app.write_text(source)
        for fast in [False, True]:
            build(compiler, app, fast)
            out = PACKAGE / "target" / ("fast-compile" if fast else "optimized")
            runner = str(out / "plenty-iced")
            result = run([runner, "--headless", "1", "1", "2"])
            assert [line for line in result.stdout.splitlines() if line.startswith("Count:")] == [
                "Count: 0", "Count: 1", "Count: 2", "Count: 0"]
            assert result.stdout.count("STATE_DESTROYED") == 1
            result = run([runner, "--headless", "999"], success=False)
            assert "unknown action ID: 999" in result.stderr
            assert "main returned iced.IcedError.InvalidArgument" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # A missing display must return a native failure and release the preflighted owner.
            no_display = {k: v for k, v in os.environ.items() if k not in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET")}
            result = run([runner], env=no_display, success=False)
            assert "main returned iced.IcedError.NativeFailure" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # Fail the actual checked addition; it must not trap or publish a new scene.
            app.write_text(source.replace("self.count = 0", "self.count = 9223372036854775807"))
            build(compiler, app, fast)
            result = run([runner, "--headless", "1"], success=False)
            assert "main returned iced.IcedError.LimitExceeded" in result.stderr
            assert result.stdout.count("---") == 1 and result.stdout.count("STATE_DESTROYED") == 1
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
            assert "main returned iced.IcedError.InvalidLayout" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # An invalid render token cannot create any nodes.
            app.write_text(source.replace("iced.Frame(token)", "iced.Frame(0u64)"))
            build(compiler, app, fast)
            result = run([runner, "--headless"], success=False)
            assert "Plenty render failed: InvalidContext" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # The process-main primitive cannot recursively enter iced from a callback.
            app.write_text(source.replace("    frame = iced.Frame", "    iced.run()?\n    frame = iced.Frame"))
            build(compiler, app, fast)
            result = run([runner, "--headless"], success=False)
            assert "Plenty render failed: InvalidContext" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # Completed UI entry is one-shot too; no second owner is created.
            app.write_text(source.replace("    iced.run()\n", "    iced.run()?\n    iced.run()\n"))
            build(compiler, app, fast)
            result = run([runner, "--headless"], success=False)
            assert "InvalidContext" in result.stderr
            assert result.stdout.count("STATE_DESTROYED") == 1
            # Observe the checked event error via the Plenty exit code, not stderr.
            observed = source[:source.index("def main()")] + """def main() -> i32:
    match iced.run():
        case Err(iced.IcedError.LimitExceeded):
            42i32
        case _:
            43i32
"""
            app.write_text(observed.replace("self.count = 0", "self.count = 1000000"))
            build(compiler, app, fast)
            with open("/dev/full", "wb") as full:
                result = subprocess.run([runner, "--headless", "1"], stderr=full, stdout=subprocess.PIPE,
                                        text=True, timeout=60)
            assert result.returncode == 42 and result.stdout.count("STATE_DESTROYED") == 1, result
            print("PASS: checked LimitExceeded reached Plenty main with full stderr; destroyed once")
            # A failed factory transfers no owner and has nothing to destroy.
            app.write_text(source.replace("Ok(State())", "Err(AllocError.CapacityOverflow)"))
            build(compiler, app, fast)
            result = run([runner, "--headless"], success=False)
            assert "main returned iced.IcedError.CapacityOverflow" in result.stderr
            assert "STATE_DESTROYED" not in result.stdout
            check_contract_rejection(compiler, app, source, fast)
            app.write_text(source)
        if args.smoke:
            build(compiler, app)
            run(["python3", "-B", str(PACKAGE / "tests/smoke_x11.py"), "--out", str(PACKAGE / "target/lifecycle-smoke")])
            assert (PACKAGE / "target/lifecycle-smoke/runner.log").read_text().count("STATE_DESTROYED") == 1
            # Unlike a headless error, this goes through iced update -> exit Task -> run -> main.
            app.write_text(source.replace("    frame.row()?", "    if state.count == 1:\n        return Err(iced.IcedError.LimitExceeded)\n    frame.row()?"))
            build(compiler, app)
            out = PACKAGE / "target/error-smoke"
            run(["python3", "-B", str(PACKAGE / "tests/smoke_x11.py"), "--out", str(out), "--expect-render-error"])
            assert (out / "runner.log").read_text().count("STATE_DESTROYED") == 1
        # Verify actual package resolution (as distinct from the launcher's
        # intentionally single-file staging workflow).
        project = root / "consumer"
        (project / "src").mkdir(parents=True)
        (project / "plenty.toml").write_text(
            'manifest-version = 1\n[package]\nname = "consumer"\nversion = "0.0.1"\n'
            f'[dependencies]\niced = {{ path = "{PACKAGE}" }}\n')
        (project / "src/main.plenty").write_text('import iced\ndef main() -> ():\n    drop(iced.Frame(0u64))\n')
        result = subprocess.run([str(compiler), "check"], cwd=project, capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
        # Unannotated bridge calls are not certified for Plenty worker entry.
        (root / "iced.plenty").write_text((PACKAGE / "src/lib.plenty").read_text())
        (root / "native.plentyi").write_text((PACKAGE / "src/native.plentyi").read_text())
        (root / "worker.plenty").write_text('import iced\n@thread_safe\nexport def entry(token: u64) -> Result[(), iced.IcedError] = "worker_entry":\n    iced.Frame(token).end()\n')
        result = run([str(compiler), "--check-module", str(root / "worker.plenty")], success=False)
        assert "thread" in result.stderr.lower(), result.stderr
    # Leave the useful uninstrumented counter selected for subsequent smoke tests.
    for fast in [False, True]:
        build(compiler, counter, fast)
    print("PASS: optimized/fast-compile state, events, errors, originating destructor, package, worker rejection")


if __name__ == "__main__":
    main()
