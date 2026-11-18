#!/usr/bin/env python3
"""Opt-in Linux GUI package qualification. No windows and no desktop input."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

EXAMPLES = Path(__file__).resolve().parent
ENV = {k: v for k, v in os.environ.items()
       if k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'XAUTHORITY')}


def run(args, cwd, expected=0, stderr=subprocess.PIPE):
    result = subprocess.run(args, cwd=cwd, env=ENV, text=True, stdout=subprocess.PIPE, stderr=stderr)
    assert result.returncode == expected, (args, result.returncode, result.stdout, result.stderr)
    return result


def qualify(compiler, toolkit, package):
    name = f'{toolkit}_counter'
    example = EXAMPLES / name
    (package / 'src').mkdir()
    for filename in ('plenty.toml', 'plenty.lock'):
        (package / filename).write_bytes((example / filename).read_bytes())
    path = package / 'src/main.plenty'
    base = (example / 'src/main.plenty').read_text().replace('    count: i64\n', '''    count: i64
    def __del__(self) -> ():
        drop(print("DROP", self.count))
''')
    egui = toolkit == 'egui'
    guard = (EXAMPLES.parents[1] / 'experiments/packages' / toolkit / 'native/expected-contract.txt').read_text().strip()
    clicks = ['20,15', '20,15', '20,36'] if egui else ['1', '1', '2']
    # Pointer move and press don't increment, release does, idle redraws don't.
    expected = [0]*3 + [1]*7 + [2]*7 + [0]*5 if egui else [0, 1, 2, 0]
    # The native bridge sees this trusted fixed header, never application-supplied C text.
    path.write_bytes((example / 'src/main.plenty').read_bytes())
    namespace = f'{toolkit}_app'
    run([compiler, '--emit-object', path, '--executable-exports', namespace, '-o', package / 'app.o'], package)
    native = EXAMPLES.parents[1] / 'experiments/packages' / toolkit / 'native'
    assert (package / f'{namespace}.h').read_bytes() == (native / 'reviewed' / f'{namespace}.h').read_bytes()
    for profile in ('optimized', 'fast-compile'):
        build = [compiler, 'build', '--locked'] + (['--fast-compile'] if profile == 'fast-compile' else [])
        exe = package / 'target' / profile / name
        path.write_text(base)
        for _ in range(2):
            run(build, package)
            result = run([exe, '--headless', *clicks], package)
            assert [int(s[7:]) for s in result.stdout.splitlines() if s.startswith('Count: ')] == expected
            assert [s for s in result.stdout.splitlines() if s.startswith('DROP ')] == ['DROP 0']
        failure = run([exe], package, expected=1)
        assert 'NativeFailure' in failure.stderr and failure.stdout.splitlines().count('DROP 0') == 1
        error = f'{toolkit}.{toolkit.capitalize()}Error'
        path.write_text(base.split('def main()')[0] + f'''def main() -> i32:
    match {toolkit}.run():
        case Err({error}.InvalidArgument):
            match {toolkit}.run():
                case Err({error}.InvalidContext):
                    42i32
                case _:
                    43i32
        case _:
            44i32
''')
        run(build, package)
        rejected = run([exe, 'document.txt'], package, expected=42)
        assert 'DROP ' not in rejected.stdout  # invalid mode precedes factory
        # Failure on a real click reaches the Plenty caller even when diagnostics fail.
        limit = 2**63-1 if egui else 1000000
        source = base.replace('self.count = 0', f'self.count = {limit}').split('def main()')[0]
        path.write_text(source + f'''def main() -> i32:
    match {toolkit}.run():
        case Err({toolkit}.{toolkit.capitalize()}Error.LimitExceeded):
            42i32
        case _:
            43i32
''')
        run(build, package)
        with open('/dev/full', 'wb') as sink:
            failure = run([exe, '--headless', clicks[0]], package, expected=42, stderr=sink)
        assert [s for s in failure.stdout.splitlines() if s.startswith('DROP ')] == [f'DROP {limit}']
        # This is valid Plenty with the same C prototype but different ownership.
        # A failed staged link must not replace the last good executable.
        callback = 'draw' if egui else 'event'
        prefix, rest = base.split(f'export def {callback}(', 1)
        signature = rest.split('\n', 1)[0].replace('&mut State', 'State')
        path.write_text(prefix + f'export def {callback}(' + signature + '\n    Ok(())\n\ndef main()' + rest.split('def main()', 1)[1])
        good = exe.read_bytes()
        failed = run(build, package, expected=1)
        assert guard in failed.stderr and ('undefined' in failed.stderr or 'not defined' in failed.stderr), failed.stderr
        assert exe.read_bytes() == good
        print(f'{toolkit} {profile}: repeated builds, click trace {expected}, owner drop, startup/checked errors, ownership mismatch rejected at final link')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plenty', type=Path, required=True)
    args = parser.parse_args()
    for toolkit in ('iced', 'egui'):
        with tempfile.TemporaryDirectory() as directory:
            qualify(args.plenty.resolve(strict=True), toolkit, Path(directory))


if __name__ == '__main__':
    main()
