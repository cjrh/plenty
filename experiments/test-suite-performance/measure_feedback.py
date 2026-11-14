#!/usr/bin/env python3
"""Measure focused test feedback in an isolated worktree, including harmless edit probes.

Each edit is an appended Rust comment, restored only if the probe still owns
those exact bytes. Do not run this while another worker edits this worktree.
This does not change Cargo profiles, target directories, coverage or caches.
"""
import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


def git(*args):
    return subprocess.check_output(['git', '-C', ROOT, *args], text=True).strip()


@contextmanager
def probe_comment(path, marker):
    original = path.read_bytes()
    modified = original + ('\n// test-feedback probe ' + marker + '\n').encode()
    path.write_bytes(modified)
    try:
        yield
    finally:
        if path.read_bytes() != modified:
            raise RuntimeError(f'{path} changed outside the probe; preserving it for inspection')
        path.write_bytes(original)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--label', required=True)
    parser.add_argument('--rounds', type=int, default=3)
    parser.add_argument('--jobs', type=int, default=6)
    parser.add_argument('--scenarios', nargs='+', choices=['warm', 'test-edit', 'compiler-edit'],
                        default=['warm', 'test-edit', 'compiler-edit'])
    args = parser.parse_args()
    if args.rounds <= 0 or args.jobs <= 0:
        parser.error('rounds/jobs must be positive')
    common = Path(git('rev-parse', '--git-common-dir'))
    if not common.is_absolute():
        common = ROOT / common
    if ROOT.resolve() == common.resolve().parent:
        parser.error('use a dedicated task worktree, not the primary checkout')
    output = ROOT / 'target/test-suite-performance' / f'{datetime.now(timezone.utc):%Y%m%dT%H%M%S}-{os.getpid()}'
    output.mkdir(parents=True)
    env = dict(os.environ, RUSTC_WRAPPER='')
    base = ['cargo', 'test', '--locked', '--workspace', '--all-features', '--test', 'integration',
            '--jobs', str(args.jobs)]
    rows = []
    data = dict(label=args.label, revision=git('rev-parse', 'HEAD'), host=platform.platform(),
                worktree=str(ROOT), environment={key: env.get(key) for key in [
                    'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_DIR', 'CARGO_BUILD_DIR',
                    'PLENTY_CACHE', 'PLENTY_CACHE_DIR', 'RUSTC_WRAPPER']},
                source_sha256={str(path): hashlib.sha256((ROOT / path).read_bytes()).hexdigest()
                               for path in ['tests/integration.rs', 'tests/support/mod.rs', 'src/frontend.rs']},
                rows=rows)

    def sample(scenario, repeat, command):
        stem = output / f'{scenario}-{repeat}'
        before = time.monotonic()
        with stem.with_suffix('.log').open('w') as log:
            result = subprocess.run(['perf', 'stat', '-x,', '-e', 'instructions:u', '-o',
                                     str(stem.with_suffix('.perf')), '--', *command],
                                    cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        perf = stem.with_suffix('.perf').read_text()
        counts = [line.split(',')[0] for line in perf.splitlines() if ',instructions:u,' in line]
        row = dict(scenario=scenario, repeat=repeat, command=command, returncode=result.returncode,
                   seconds=time.monotonic() - before,
                   instructions=int(counts[0]) if counts and counts[0].isdigit() else None)
        rows.append(row)
        (output / 'results.json').write_text(json.dumps(data, indent=2) + '\n')
        print(json.dumps(row), flush=True)
        if result.returncode:
            raise RuntimeError(f'{scenario} failed; inspect {stem.with_suffix(".log")}')

    # Populate artifacts once; excluded from all scenario comparisons.
    with (output / 'prepare.log').open('w') as log:
        subprocess.run([*base, '--no-run'], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        # Cargo artifacts and Plenty's compiled-function cache are separate.
        # Execute the selected fixtures once before calling a sample warm.
        if 'warm' in args.scenarios:
            subprocess.run([*base, 'test_arithmetic_errors::'], cwd=ROOT, env=env,
                           stdout=log, stderr=subprocess.STDOUT, check=True)
    for scenario in args.scenarios:
        for repeat in range(args.rounds):
            if scenario == 'warm':
                sample(scenario, repeat, [*base, 'test_arithmetic_errors::'])
            else:
                relative = 'tests/test_arithmetic_errors.rs' if scenario == 'test-edit' else 'src/frontend.rs'
                with probe_comment(ROOT / relative, f'{args.label}-{scenario}-{repeat}'):
                    sample(scenario, repeat, [*base, '--no-run'])
    print(f'Results: {output / "results.json"}')


if __name__ == '__main__':
    main()
