#!/usr/bin/env python3
"""Coordinate concurrent ordinary package builds without compiling or running GUI code."""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

TRIPLE = 'x86_64-unknown-linux-gnu'
REPO = Path(__file__).resolve().parents[2]


def cargo():
    env = os.environ
    backend = env['BACKEND']
    name = f'plenty_{backend}'
    native = Path(env['REPO']) / 'experiments/packages' / backend / 'native'
    assert Path.cwd() == Path('/') and env['CARGO_NET_OFFLINE'] == 'true'
    assert env['CARGO_BUILD_TARGET'] != TRIPLE
    assert env[f'PLENTY_{backend.upper()}_APP_DIR'] == str(native / 'reviewed')
    args = sys.argv[1:]
    assert args[0] == 'rustc' and '--locked' in args and '--lib' in args
    for flag, expected in [('--target', TRIPLE), ('--target-dir', Path(env['REPO']) / 'target/std-gui' / backend),
                           ('--manifest-path', native / 'Cargo.toml'), ('--features', env['EXPECTED_FEATURES'])]:
        assert args.count(flag) == 1 and args[args.index(flag) + 1] == str(expected), args
    marker = env['MARKER']
    # Same pathname for successive invocations, but deliberately different bytes and args.
    archive = Path(env['CONTROL']) / f'{backend}.a'
    archive.write_text(marker)
    record = {'manifest_path': str(native / 'Cargo.toml'), 'target': {'name': name, 'crate_types': ['staticlib']}}
    print(json.dumps({**record, 'reason': 'compiler-artifact', 'filenames': [str(archive)]}))
    print(json.dumps({**record, 'reason': 'compiler-message', 'message': {'message': f'native-static-libs: -l{marker} -lsecond'}}))


def driver():
    if '-Wl,--version' in sys.argv:
        return 1
    root = Path(os.environ['CONTROL'])
    marker = os.environ['MARKER']
    if marker == 'A':
        (root / 'driver-ready').touch()
        wait_for(lambda: (root / 'driver-release').exists())
    if os.environ['BACKEND']:
        archive = str(root / (os.environ['BACKEND'] + '.a'))
        assert archive in sys.argv and Path(archive).read_text() == marker, (marker, sys.argv)
        assert sys.argv[sys.argv.index('-lsecond') - 1] == '-l' + marker, sys.argv
    Path(sys.argv[sys.argv.index('-o') + 1]).write_text(marker)
    return 0


def wait_for(condition):
    deadline = time.monotonic() + 45
    while not condition():
        if time.monotonic() >= deadline:
            raise TimeoutError('build interleaving barrier')
        time.sleep(.025)


def member(parent, name, backend, binary):
    directory = parent / name
    (directory / 'src').mkdir(parents=True)
    source = ((REPO / f'examples/gui/{backend}_counter/src/main.plenty').read_text()
              if backend else 'def main() -> ():\n    pass\n')
    (directory / 'src/main.plenty').write_text(source)
    (directory / 'plenty.toml').write_text(f'manifest-version = 1\n[package]\nname = "{name}"\nversion = "0.1.0"\n'
                                          f'[[bin]]\nname = "{binary}"\npath = "src/main.plenty"\n')
    return directory


def exercise(root, compiler, features):
    workspace = root / 'workspace'
    a = member(workspace, 'gui', 'iced', 'shared')
    cli = member(workspace, 'cli', '', 'shared')
    independent = member(workspace, 'independent', 'egui', 'other')
    (workspace / 'plenty.toml').write_text('manifest-version = 1\n[workspace]\nmembers = ["gui", "cli", "independent"]\n')
    b = member(root, 'other_consumer', 'iced', 'app')
    for tool in ('cargo', 'cc'):
        shutil.copy(__file__, root / tool)
        (root / tool).chmod(0o755)
    env = {**os.environ, 'CONTROL': str(root), 'REPO': str(REPO), 'PATH': str(root) + ':' + os.environ['PATH'],
           'CARGO_BUILD_TARGET': 'wasm32-unknown-unknown', 'CARGO_NET_OFFLINE': 'false',
           'EXPECTED_FEATURES': features, 'PLENTY_CACHE_DIR': str(root / 'cache')}
    with ExitStack() as stack:
        def start(directory, backend, marker):
            log = root / f'{marker}.log'
            output = stack.enter_context(log.open('w'))
            p = subprocess.Popen([compiler, 'build'], cwd=directory, env={**env, 'MARKER': marker, 'BACKEND': backend},
                                 stdout=output, stderr=output, start_new_session=True)
            def stop():
                if p.poll() is None:
                    os.killpg(p.pid, signal.SIGKILL)
                    p.wait()
            stack.callback(stop)
            return p, log

        def finished(job):
            p, log = job
            assert p.wait(timeout=45) == 0, log.read_text()
            return log.read_text()

        def waiting(job, path):
            p, log = job
            def at_lock():
                assert p.poll() is None, log.read_text()
                return f'Waiting for build lock {path}' in log.read_text()
            wait_for(at_lock)

        first = start(a, 'iced', 'A')
        wait_for(lambda: (root / 'driver-ready').exists())
        second = start(b, 'iced', 'B')
        waiting(second, REPO / 'target/std-gui/iced/plenty-link.lock')
        contested = start(cli, '', 'CLI')
        waiting(contested, workspace / 'target/optimized/.build-locks/bin-shared')
        # No global native, workspace, or profile lock: different backend AND
        # different output finish while the first two contenders are blocked.
        finished(start(independent, 'egui', 'D'))
        assert (workspace / 'target/optimized/other').read_text() == 'D'
        (root / 'driver-release').touch()
        for job in (first, second, contested):
            finished(job)
        assert (b / 'target/optimized/app').read_text() == 'B'
        output = workspace / 'target/optimized/shared'
        assert output.read_text() == 'CLI'
        assert 'Fresh' in finished(start(cli, '', 'NoRelink')) and output.read_text() == 'CLI'
        finished(start(a, 'iced', 'GUI'))
        assert output.read_text() == 'GUI'
        assert 'Fresh' not in finished(start(cli, '', 'CLI2')) and output.read_text() == 'CLI2'
    print(f'PASS: archive bytes and argument order, output/fingerprint exclusion, independent builds; {features}, {TRIPLE}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plenty', required=True, type=Path)
    parser.add_argument('--runtime-checks', action='store_true', help='expect a runtime-checks compiler')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as directory:
        exercise(Path(directory), args.plenty.resolve(strict=True),
                 'application,plenty-runtime/allocation-checks' if args.runtime_checks else 'application')


if __name__ == '__main__':
    if Path(sys.argv[0]).name == 'cargo':
        cargo()
    elif Path(sys.argv[0]).name == 'cc':
        sys.exit(driver())
    else:
        main()
