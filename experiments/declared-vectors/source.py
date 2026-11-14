#!/usr/bin/env python3
"""Generate matched safe-source fixed-array Vec3/scalar n-body probes."""
import argparse
import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('nbody_source', ROOT / 'experiments/nbody-structural/generate.py')
old = importlib.util.module_from_spec(spec)
spec.loader.exec_module(old)


def generate(output):
    output.mkdir(parents=True, exist_ok=True)
    old.plenty(output)
    production = (ROOT / 'benchmarks/game/n-body/plenty/nbody.plenty').read_text()
    scalar = (output / 'locals.plenty').read_text()
    scalar = scalar.replace('list[Body]', 'array[Body, 5]')
    scalar = scalar.replace('def starting_state() -> Result[array[Body, 5], AllocError]:',
                            'def starting_state() -> array[Body, 5]:')
    scalar = scalar.replace('    ]?\n', '    ]\n')
    scalar = scalar.replace('mut bodies = starting_state()?', 'mut bodies = starting_state()')
    start = scalar.index('def advance(')
    header = scalar[start:scalar.index('\n', start)]
    fields = ('x', 'y', 'z', 'vx', 'vy', 'vz', 'mass')
    # Print complete state as well as both energy values for differential checks.
    final = scalar.rindex('    Ok(())')
    fmt = ' '.join('{b.' + field + ':.17e}' for field in fields)
    scalar = scalar[:final] + ('    if len(argv) > 2:\n        for b in &bodies:\n'
                              f'            print(f"{fmt}"?)?\n') + scalar[final:]
    (output / 'array-scalar.plenty').write_text(scalar)
    final_production = production.rindex('    Ok(())')
    production = production[:final_production] + ('    if len(argv) > 2:\n        for b in &bodies:\n'
                    f'            print(f"{fmt}"?)?\n') + production[final_production:]
    (output / 'production.plenty').write_text(production)
    # Recompute the replace region because adding output only extends main.
    start = scalar.index('def advance(')
    end = scalar.index('\n\ntype AppError', start)
    lines = [header]
    for i in range(5):
        lines += [f'    mut p{i} = bodies[{i}].position',
                  f'    mut v{i} = bodies[{i}].velocity', f'    mass{i} = bodies[{i}].mass']
    lines += ['    dtv = simd.f64x2.Lanes(dt, dt)', '    for _ in range(steps):']
    # Reuse locals to stay below the existing 256-local per-function limit.
    seen = set()
    def bind(name, expr):
        prefix = 'mut ' if name not in seen else ''
        seen.add(name)
        lines.append(f'        {prefix}{name} = {expr}')
    for offset in range(0, len(old.PAIRS), 2):
        pairs = old.PAIRS[offset:offset + 2]
        for lane, (i, j) in enumerate(pairs):
            bind('d' + str(lane), f'p{i} - p{j}')
            bind('s' + str(lane), f'd{lane}.dot(&d{lane})')
        bind('squared', 'simd.f64x2.Lanes(s0, s1)')
        bind('magnitudes', 'dtv.div(&squared.mul(&squared.sqrt()))')
        bind('m0, m1', 'magnitudes.to_tuple()')
        for lane, (i, j) in enumerate(pairs):
            bind('mi', f'mass{i} * m{lane}')
            bind('mj', f'mass{j} * m{lane}')
            lines += [f'        v{i} = v{i} - d{lane} * mj',
                      f'        v{j} = v{j} + d{lane} * mi']
    for i in range(5):
        lines.append(f'        p{i} = p{i} + v{i} * dt')
    for i in range(5):
        lines += [f'    bodies[{i}].position = p{i}', f'    bodies[{i}].velocity = v{i}']
    lines.append('    Ok(())')
    vector = scalar[:start] + '\n'.join(lines) + '\n' + scalar[end:]
    vector = vector.replace('    x: f64\n    y: f64\n    z: f64\n    vx: f64\n    vy: f64\n    vz: f64\n',
                            '    position: Vec3\n    velocity: Vec3\n')
    vector = vector.replace('Body(x, y, z, vx * DAYS_PER_YEAR, vy * DAYS_PER_YEAR, vz * DAYS_PER_YEAR, mass * SOLAR_MASS)',
                            'Body(Vec3(x, y, z), Vec3(vx * DAYS_PER_YEAR, vy * DAYS_PER_YEAR, vz * DAYS_PER_YEAR), mass * SOLAR_MASS)')
    vector = vector.replace('Body(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR_MASS)',
                            'Body(Vec3(0.0, 0.0, 0.0), Vec3(0.0, 0.0, 0.0), SOLAR_MASS)')
    import re
    vector = re.sub(r'\.(vx|vy|vz)\b', lambda m: '.velocity.' + m[1][1:], vector)
    vector = re.sub(r'(?<!position)\.(x|y|z)\b', lambda m: '.position.' + m[1], vector)
    # The second rewrite also visits newly generated velocity.x: fix its prefix.
    vector = vector.replace('.velocity.position.', '.velocity.')
    imports = 'from std.vector import Vec3\nimport std.simd as simd\n'
    (output / 'array-vec3.plenty').write_text(imports + vector)
    unpaired = re.sub(
        r'        (?:mut )?squared = simd.f64x2.Lanes\(s0, s1\)\n'
        r'        (?:mut )?magnitudes = dtv.div\(&squared.mul\(&squared.sqrt\(\)\)\)\n'
        r'        (mut )?m0, m1 = magnitudes.to_tuple\(\)',
        lambda match: ('        ' + (match[1] or '') + 'm0 = dt / (s0 * s0.sqrt())\n'
                       '        ' + (match[1] or '') + 'm1 = dt / (s1 * s1.sqrt())'), vector)
    (output / 'array-vec3-unpaired.plenty').write_text(imports + unpaired)
    scalar_api = (ROOT / 'stdlib/std/vector.plenty').read_text().replace('pub ', '')
    (output / 'array-ordinary.plenty').write_text('import std.simd as simd\n' + scalar_api + '\n' + vector)

    common = """from std.vector import Vec3

def owned(value: Vec3) -> Vec3:
    mut current = value
    step = Vec3(0.25, -0.5, 0.125)
    for i in range(4):
        current = (current + step) * 0.5
    current

def shared(value: &Vec3) -> Vec3:
    mut current = *value
    step = Vec3(0.25, -0.5, 0.125)
    for i in range(4):
        current = (current + step) * 0.5
    current

"""
    for kind in ('local', 'owned', 'shared', 'array'):
        body = ['def advance(count: i64) -> Vec3:', '    mut current = Vec3(1.0, 2.0, 3.0)',
                '    step = Vec3(0.25, -0.5, 0.125)']
        if kind == 'array':
            body.append('    mut cells: array[Vec3, 1] = [current]')
        body.append('    for i in range(count):')
        if kind == 'local':
            body.extend(['        current = (current + step) * 0.5'] * 8)
        elif kind == 'array':
            for _ in range(2):
                body += ['        current = cells[0]']
                body.extend(['        current = (current + step) * 0.5'] * 4)
                body += ['        cells[0] = current']
        else:
            argument = '&current' if kind == 'shared' else 'current'
            body += [f'        current = {kind}({argument})'] * 2
        body += ['    current', '', 'def main() -> Result[(), AllocError | IoError | ParseError]:',
                 '    argv = args()?', '    count = i64.parse(argv[1])?',
                 '    print(advance(count).to_tuple())?', '    Ok(())', '']
        (output / ('cost-' + kind + '.plenty')).write_text(common + '\n'.join(body))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    generate(parser.parse_args().output)


if __name__ == '__main__':
    main()
