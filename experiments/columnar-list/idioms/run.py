#!/usr/bin/env python3
"""Compare the tutorial's exact list/clist idioms with checked optimized builds."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
PAGE = ROOT / 'book/src/tutorial/special-purpose/columnar-lists/index.md'


def sources(rows, width, repeats, samples):
    page = PAGE.read_text()
    model = re.search(r'```plenty-file particles.plenty\n(.*?)```', page, re.S)[1]
    schema = model.split('\npub def ')[0].rstrip()
    schema += ''.join(f'\n    pub unused_{i}: f64' for i in range(width - 8)) + '\n'
    examples = re.findall(r'```plenty-file (?:aggregate|conditional|advance)\.plenty\n(.*?)```', page, re.S)
    # The measured functions are extracted verbatim, rather than independently
    # reimplementing something that merely resembles the documented idiom.
    kernels = '\n\n'.join(e.split('\n', 1)[1] for e in examples)
    source = 'from particles import Particle\nfrom std.time import monotonic_now\n' + kernels
    for layout in ('list', 'clist'):
        for task, args in [('mass', ''), ('hot_mass', ', 20.0'), ('advance', ', 0.25')]:
            mutable = task == 'advance'
            source += f'\n\ndef measure_{layout}_{task}(particles: &{"mut " if mutable else ""}{layout}[Particle], sample: i64) -> Result[(), IoError]:\n'
            source += '    begin = monotonic_now()\n'
            if not mutable:
                source += '    mut checksum = 0.0\n'
            source += f'    for repeat in range({repeats}):\n'
            source += f'        {"" if mutable else "checksum += "}{layout}_{task}(particles{args})\n'
            source += '    end = monotonic_now()\n'
            if mutable:
                source += '    mut checksum = 0.0\n'
                if layout == 'list':
                    source += '    for particle in particles:\n        checksum += particle.x + particle.y\n'
                else:
                    source += '    for i in range(len(particles)):\n        checksum += particles[i].x + particles[i].y\n'
            source += f'    print("{layout}_{task}", sample, end.saturating_duration_since(&begin).as_nanoseconds(), checksum)?\n    Ok(())\n'
    values = ['n', 'n + 1.0', '0.5', '1.0', 'n + 2.0', 'f64(i % 40)', '100i32', '0i32']
    values += ['n'] * (width - 8)
    source += f'\n\ndef main() -> Result[(), AllocError | AppendError[Particle] | IoError | OverflowError[i64] | ZeroDivisionError]:\n'
    for layout in ('list', 'clist'):
        source += f'    mut {layout}_rows = {layout}[Particle].with_capacity({rows})?\n'
        source += f'    for i in range({rows}):\n        n = f64(i % 1024)\n        {layout}_rows.append(Particle({", ".join(values)}))?\n'
    source += f'    for sample in range({samples + 1}):\n'
    for task in ('mass', 'hot_mass', 'advance'):
        source += '        if sample % 2 == 0:\n'
        for layout in ('list', 'clist'):
            source += f'            measure_{layout}_{task}(&{"mut " if task == "advance" else ""}{layout}_rows, sample)?\n'
        source += '        else:\n'
        for layout in ('clist', 'list'):
            source += f'            measure_{layout}_{task}(&{"mut " if task == "advance" else ""}{layout}_rows, sample)?\n'
    return schema, source + '    Ok(())\n'


def parse(output, rows, repeats, samples):
    data = {}
    mass = sum(i % 1024 + 2 for i in range(rows)) * repeats
    hot = sum(i % 1024 + 2 for i in range(rows) if i % 40 >= 20) * repeats
    initial = sum(2 * (i % 1024) + 1 for i in range(rows))
    for line in output.splitlines():
        name, sample, ns, checksum = line.split()
        sample, ns, checksum = int(sample), int(ns), float(checksum)
        task = name.split('_', 1)[1]
        expected = {'mass': mass, 'hot_mass': hot,
                    'advance': initial + (sample + 1) * repeats * rows * 0.375}[task]
        if checksum != expected:
            raise ValueError(f'{name} sample {sample}: {checksum} != {expected}')
        data.setdefault(name, []).append({'sample': sample, 'ns': ns, 'checksum': checksum})
    if len(data) != 6 or any([d['sample'] for d in v] != list(range(samples + 1)) for v in data.values()):
        raise ValueError('missing or duplicated samples')
    summary = {}
    for task in ('mass', 'hot_mass', 'advance'):
        timings = {layout: [d['ns'] / (rows * repeats) for d in data[f'{layout}_{task}'][1:]]
                   for layout in ('list', 'clist')}
        ratios = [a / b for a, b in zip(timings['list'], timings['clist'])]
        summary[task] = {'ns_per_row': {k: statistics.median(v) for k, v in timings.items()},
                         'paired_speedup_median': statistics.median(ratios),
                         'paired_speedup_range': [min(ratios), max(ratios)]}
    return data, summary


def build_processes():
    names = subprocess.check_output(['ps', '-eo', 'comm=,stat='], text=True)
    return [line for line in names.splitlines()
            if not line.split()[-1].startswith('Z')
            and (line.split()[0] in {'cargo', 'rustc', 'cc', 'ld', 'ld.lld'}
                 or line.split()[0].startswith('integration-'))]


def execute(command, require_idle):
    if not require_idle:
        return subprocess.run(command, check=True, capture_output=True, text=True)
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        active = build_processes()
        if active:
            print('Waiting for builds/tests: ' + ', '.join(active), flush=True)
            time.sleep(5)
            continue
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as process:
            interference = []
            while True:
                try:
                    stdout, stderr = process.communicate(timeout=0.25)
                    break
                except subprocess.TimeoutExpired:
                    interference.extend(build_processes())
            interference.extend(build_processes())
            if process.returncode:
                raise subprocess.CalledProcessError(process.returncode, command, stdout, stderr)
        if not interference:
            return subprocess.CompletedProcess(command, 0, stdout, stderr)
        print('Discarding run overlapping builds/tests: ' + ', '.join(sorted(set(interference))), flush=True)
    raise RuntimeError('No build/test-free measurement window within ten minutes')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('compiler', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--rows', type=int, nargs='+', default=[8192, 262144])
    parser.add_argument('--widths', type=int, nargs='+', default=[8, 64])
    parser.add_argument('--repeats', type=int, default=50)
    parser.add_argument('--samples', type=int, default=7)
    parser.add_argument('--cpu', type=int, default=8)
    parser.add_argument('--verify-only', action='store_true')
    parser.add_argument('--require-idle', action='store_true', help='reject runs overlapping visible build/test processes; use host process visibility')
    args = parser.parse_args()
    if min(args.rows + [args.repeats, args.samples]) < 1 or min(args.widths) < 8:
        parser.error('positive sizes/repeats/samples and at least eight fields required')
    compiler, out = args.compiler.resolve(), args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    results = {'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
               'compiler_sha256': hashlib.sha256(compiler.read_bytes()).hexdigest(),
               'tutorial_kernels_sha256': hashlib.sha256(sources(1, 8, 1, 1)[1].split('\n\ndef measure_')[0].encode()).hexdigest(),
               'measured_at_utc': datetime.now(timezone.utc).isoformat(),
               'compiler_version': subprocess.check_output([str(compiler), '--version'], text=True).strip(),
               'platform': platform.platform(), 'cpu': args.cpu,
               'repeats': args.repeats, 'samples': args.samples,
               'idle_monitor': args.require_idle, 'cases': []}
    cpuinfo = Path('/proc/cpuinfo').read_text()
    results['cpu_model'] = next(line.split(':', 1)[1].strip() for line in cpuinfo.splitlines() if line.startswith('model name'))
    env = dict(os.environ, PLENTY_CACHE='off')
    prepared = []
    # Finish all compilation before measuring any case.
    for width in args.widths:
        for rows in args.rows:
            directory = out / f'w{width}-n{rows}'
            directory.mkdir(exist_ok=True)
            schema, source = sources(rows, width, args.repeats, args.samples)
            (directory / 'particles.plenty').write_text(schema)
            program = directory / 'main.plenty'
            program.write_text(source)
            binary = directory / 'program'
            subprocess.run([str(compiler), '--no-strip', '--compile', str(program), '-o', str(binary)], check=True, env=env)
            prepared.append((width, rows, program, binary))
    for width, rows, program, binary in prepared:
        command = ['taskset', '-c', str(args.cpu), str(binary)]
        # Whole-process instructions include construction, checksums and teardown;
        # they are supporting evidence, not a per-task speedup measurement.
        if not args.verify_only:
            command = ['perf', 'stat', '-x', ';', '-e', 'instructions:u', '--', *command]
        run = execute(command, args.require_idle)
        raw, summary = parse(run.stdout, rows, args.repeats, args.samples)
        counters = re.search(r'^([0-9]+);[^\n]*;instructions:u;', run.stderr, re.M)
        case = {'width': width, 'rows': rows, 'source_sha256': hashlib.sha256(program.read_bytes()).hexdigest(),
                'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'whole_process_instructions': int(counters[1]) if counters else None,
                'raw': raw, 'summary': summary}
        results['cases'].append(case)
        (out / 'measurements.json').write_text(json.dumps(results, indent=2) + '\n')
        print(width, rows, json.dumps(summary), flush=True)


if __name__ == '__main__':
    main()
