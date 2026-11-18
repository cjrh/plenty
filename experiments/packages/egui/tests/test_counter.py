#!/usr/bin/env python3
"""Build/link/load the real Plenty counter; only the private smoke test opens a display."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

PACKAGE = Path(__file__).resolve().parents[1]
COUNTER = PACKAGE / 'examples/counter.plenty'
ENV = {k: v for k, v in os.environ.items() if k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'XAUTHORITY')}
INC, RESET = '20,15', '20,36'


def run(command, *, success=True, cwd=PACKAGE, stderr=subprocess.PIPE):
    p = subprocess.run(command, cwd=cwd, env=ENV, stdout=subprocess.PIPE, stderr=stderr,
                       text=True, timeout=600)
    if (p.returncode == 0) != success:
        raise AssertionError(f'{command}: {p.returncode}\n{p.stdout}\n{p.stderr}')
    return p


def build(compiler, app, fast=False, *, success=True):
    return run(['python3', '-B', PACKAGE / 'build.py', '--plenty', compiler, app,
                *(['--fast-compile'] if fast else [])], success=success)


def contracts(compiler, app, source, fast, header):
    good = header.read_text().splitlines()
    [prototype] = [s for s in good if s.startswith('uint32_t egui_app_draw(')]
    [guard] = [s[5:-7] for s in good if s.startswith('void egui_app_plenty_contract_v1_')]
    start = source.index('export def draw(')
    signature = source[start:source.index('\n', start)]
    for name, variant in [('consuming draw', source[:start] + signature.replace('&mut State', 'State') +
                           '\n    Ok(())\n\n' + source[source.index('def main()'):]),
                          ('extra export', source + '\nexport def extra() -> u32 = "egui_app_extra":\n    0u32\n')]:
        app.write_text(variant)
        # Never execute a mismatch. It must fail for THIS missing guard at the final link.
        result = build(compiler, app, fast, success=False)
        candidate = header.read_text()
        assert prototype in candidate.splitlines() and guard not in candidate
        assert any(guard in line and ('not defined' in line or 'undefined' in line) for line in result.stderr.splitlines()), result.stderr
        print(f'PASS: rejected {name}, identical C draw prototype, missing {guard}')


def suite(compiler, app, source, fast, smoke):
    profile = 'fast-compile' if fast else 'optimized'
    out = PACKAGE / 'target' / profile
    runner = out / 'plenty-egui'
    headless = [runner, '--headless']

    def select(text):
        app.write_text(text)
        build(compiler, app, fast)

    def check(result, *, destroyed, error=None):
        drops = [line for line in result.stdout.splitlines() if line.startswith('DROP ')]
        assert drops == ([] if destroyed is None else [f'DROP {destroyed}']), result
        if error:
            assert f'main returned egui.EguiError.{error}' in result.stderr, result

    select(source)
    result = run(headless + [INC, INC, RESET])
    # Initial frame, then for each click: move, press, release, four idle frames.
    counts = [int(s[7:]) for s in result.stdout.splitlines() if s.startswith('Count: ')]
    expected = [0] + [0, 0] + [1]*5 + [1, 1] + [2]*5 + [2, 2] + [0]*5
    assert counts == expected, counts
    check(result, destroyed=0)
    check(run([runner], success=False), destroyed=0, error='NativeFailure')  # no display

    full = source.replace('self.count = 0', 'self.count = i64.MAX')
    select(full)
    check(run(headless + [INC], success=False), destroyed=2**63-1, error='LimitExceeded')
    # A different failure *after* a successful mutation need not undo that change.
    partial = source.replace('    label = ', '    if state.count == 1:\n        return Err(egui.EguiError.LimitExceeded)\n    label = ')
    select(partial)
    check(run(headless + [INC], success=False), destroyed=1, error='LimitExceeded')
    if smoke:
        directory = out / 'error-smoke'
        run(['python3', '-B', PACKAGE / 'tests/smoke_x11.py', '--binary', runner, '--out', directory, '--expect-render-error'])
        lines = (directory / 'runner.log').read_text().splitlines()
        assert [s for s in lines if s.startswith('DROP ')] == ['DROP 1'], lines

    observed = full[:full.index('def main()')] + '''def main() -> i32:
    match egui.run():
        case Err(egui.EguiError.LimitExceeded):
            42i32
        case _:
            43i32
'''
    select(observed)
    with open('/dev/full', 'wb') as sink:
        result = run(headless + [INC], stderr=sink, success=False)
    assert result.returncode == 42, result
    check(result, destroyed=2**63-1)
    with open('/dev/full', 'wb') as sink:
        muted = subprocess.run(headless + [INC], stdout=sink, stderr=sink, env=ENV, timeout=60)
    assert muted.returncode == 42, muted
    for old, new, error, destroyed in [
        ('Ok(State())', 'Err(AllocError.CapacityOverflow)', 'CapacityOverflow', None),
        ('egui.Ui(token)', 'egui.Ui(0u64)', 'InvalidContext', 0),
        ('    ui = ', '    egui.run()?\n    ui = ', 'InvalidContext', 0),
        ('    egui.run()\n', '    egui.run()?\n    egui.run()\n', 'InvalidContext', 0),
    ]:
        select(source.replace(old, new))
        check(run(headless, success=False), destroyed=destroyed, error=error)
    if smoke:
        instrumented = source.replace('state.increment().map_error(count_limit)?',
                                      'state.increment().map_error(count_limit)?\n        drop(print("MUTATION", state.count))')
        select(instrumented.replace('state.count = 0', 'state.count = 0\n        drop(print("MUTATION", state.count))'))
        directory = out / 'lifecycle-smoke'
        run(['python3', '-B', PACKAGE / 'tests/smoke_x11.py', '--binary', runner, '--out', directory])
        lines = (directory / 'runner.log').read_text().splitlines()
        assert [s for s in lines if s.startswith('MUTATION ')] == ['MUTATION 1', 'MUTATION 2', 'MUTATION 0'], lines
        assert [s for s in lines if s.startswith('DROP ')] == ['DROP 0'], lines
    contracts(compiler, app, source, fast, out / 'egui_app.h')
    build(compiler, COUNTER, fast)  # restore the normal app after build-only negative tests
    print(f'PASS {profile}: actual painted counts {counts}; factory/draw errors, stderr failure, owner cleanup, one-shot')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plenty', type=Path, default=PACKAGE.parents[2] / 'target/release/plenty')
    parser.add_argument('--smoke', action='store_true', help='also verify lifecycle on private X11')
    args = parser.parse_args()
    compiler = args.plenty.resolve(strict=True)
    source = COUNTER.read_text().replace('    count: i64\n', '''    count: i64
    def __del__(self) -> ():
        drop(print("DROP", self.count))
''')
    with tempfile.TemporaryDirectory(prefix='plenty-egui-test-') as directory:
        root = Path(directory)
        for fast in (False, True):
            suite(compiler, root / 'app.plenty', source, fast, args.smoke)
        (root / 'src').mkdir()
        (root / 'plenty.toml').write_text('manifest-version = 1\n[package]\nname = "consumer"\nversion = "0.0.1"\n'
                                          f'[dependencies]\negui = {{ path = "{PACKAGE}" }}\n')
        (root / 'src/main.plenty').write_text('import egui\ndef main() -> ():\n    drop(egui.Ui(0u64))\n')
        run([compiler, 'check'], cwd=root)
        (root / 'egui.plenty').write_bytes((PACKAGE / 'src/lib.plenty').read_bytes())
        (root / 'native.plentyi').write_bytes((PACKAGE / 'src/native.plentyi').read_bytes())
        worker = root / 'worker.plenty'
        worker.write_text('import egui\n@thread_safe\nexport def entry(token: u64) -> Result[bool, egui.EguiError] = "worker_entry":\n    egui.Ui(token).button(&"x")\n')
        result = run([compiler, '--check-module', worker], success=False)
        assert 'thread' in result.stderr.lower(), result.stderr
    print('PASS: package resolution and worker rejection')


if __name__ == '__main__':
    main()
