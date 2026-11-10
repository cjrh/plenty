#!/usr/bin/env python3
"""Generate n-body structural probes without changing the benchmark."""
from pathlib import Path
import argparse
import re

ROOT = Path(__file__).resolve().parents[2]
FIELDS = ('x', 'y', 'z', 'vx', 'vy', 'vz', 'mass')
PAIRS = [(i, j) for i in range(5) for j in range(i + 1, 5)]


def plenty(out):
    source = (ROOT / 'benchmarks/game/n-body/plenty/nbody.plenty').read_text()
    start = source.index('def advance(')
    end = source.index('\n\ntype AppError', start)
    header = source[start:source.index('\n', start)]
    fused = '''    n = len(bodies)
    for _ in range(steps):
        for i in range(n):
            for j in range(i + 1, n):
                dx = bodies[i].x - bodies[j].x
                dy = bodies[i].y - bodies[j].y
                dz = bodies[i].z - bodies[j].z
                d2 = dx * dx + dy * dy + dz * dz
                mag = dt / (d2 * d2.sqrt())
                mi = bodies[i].mass * mag
                mj = bodies[j].mass * mag
                bodies[i].vx -= dx * mj
                bodies[i].vy -= dy * mj
                bodies[i].vz -= dz * mj
                bodies[j].vx += dx * mi
                bodies[j].vy += dy * mi
                bodies[j].vz += dz * mi
        for i in range(n):
            bodies[i].x += dt * bodies[i].vx
            bodies[i].y += dt * bodies[i].vy
            bodies[i].z += dt * bodies[i].vz
    Ok(())
'''
    (out / 'fused.plenty').write_text(source[:start] + header + '\n' + fused + source[end:])
    # Reuse explicit mutable temporaries to distinguish the old source-expansion
    # slot-limit failure from the backend's ability to keep fixed state in SSA.
    lines = [header]
    for i in range(5):
        for f in FIELDS:
            lines.append(f'    mut {f}{i} = bodies[{i}].{f}')
    for name in ('dx', 'dy', 'dz', 'd2', 'mag', 'mi', 'mj'):
        lines.append(f'    mut {name} = 0.0')
    lines.append('    for _ in range(steps):')
    for i, j in PAIRS:
        for f in 'xyz':
            lines.append(f'        d{f} = {f}{i} - {f}{j}')
        lines += ['        d2 = dx * dx + dy * dy + dz * dz',
                  '        mag = dt / (d2 * d2.sqrt())',
                  f'        mi = mass{i} * mag', f'        mj = mass{j} * mag']
        for f in 'xyz':
            lines.append(f'        v{f}{i} -= d{f} * mj')
        for f in 'xyz':
            lines.append(f'        v{f}{j} += d{f} * mi')
    for i in range(5):
        for f in 'xyz':
            lines.append(f'        {f}{i} += dt * v{f}{i}')
    for i in range(5):
        for f in FIELDS[:-1]:
            lines.append(f'    bodies[{i}].{f} = {f}{i}')
    lines.append('    Ok(())')
    (out / 'locals.plenty').write_text(source[:start] + '\n'.join(lines) + '\n' + source[end:])


def simd(out):
    source = (out / 'locals.plenty').read_text()
    prefix, rest = source.split('    for _ in range(steps):', 1)
    _, suffix = rest.split('        x0 += dt * vx0', 1)
    prefix = 'import std.simd as simd\n' + prefix
    for name in ('dx', 'dy', 'dz', 'd2', 'mag', 'mi', 'mj'):
        prefix = prefix.replace(f'    mut {name} = 0.0\n', '')
    body = ['    for _ in range(steps):']
    for k in range(0, 10, 2):
        group = PAIRS[k:k+2]
        for lane, (i, j) in enumerate(group):
            for f in 'xyz':
                body.append(f'        d{f}{lane} = {f}{i} - {f}{j}')
        for f in 'xyz':
            body.append(f'        d{f}v = simd.f64x2.Lanes(d{f}0, d{f}1)')
        body += ['        d2v = dxv.mul(&dxv).add(&dyv.mul(&dyv)).add(&dzv.mul(&dzv))',
                 '        mv = simd.f64x2.Lanes(dt, dt).div(&d2v.mul(&d2v.sqrt()))',
                 '        m0, m1 = mv.to_tuple()']
        for lane, (i, j) in enumerate(group):
            body += [f'        mi{lane} = mass{i} * m{lane}',
                     f'        mj{lane} = mass{j} * m{lane}']
            for f in 'xyz':
                body.append(f'        v{f}{i} -= d{f}{lane} * mj{lane}')
            for f in 'xyz':
                body.append(f'        v{f}{j} += d{f}{lane} * mi{lane}')
    # Block-scoped bindings still consume slots; unique names are not needed
    # here because mutable temporaries can be reused across the five groups.
    seen = set()
    for i, line in enumerate(body):
        if ' = ' in line:
            lhs, rhs = line.strip().split(' = ', 1)
            if lhs not in seen:
                seen.add(lhs)
                # Tuple assignment cannot redeclare after the first group.
                body[i] = '        mut ' + lhs + ' = ' + rhs
    (out / 'simd-locals.plenty').write_text(prefix + '\n'.join(body) +
        '\n        x0 += dt * vx0' + suffix)


def simd_xy(out):
    source = (ROOT / 'benchmarks/game/n-body/plenty/nbody.plenty').read_text()
    start = source.index('def advance(')
    end = source.index('\n\ntype AppError', start)
    lines = [source[start:source.index('\n', start)]]
    for i in range(5):
        for v, fields in [('p', ('x', 'y')), ('v', ('vx', 'vy'))]:
            a, b = fields
            lines.append(f'    mut {v}{i} = simd.f64x2.Lanes(bodies[{i}].{a}, bodies[{i}].{b})')
        for f in ('z', 'vz', 'mass'):
            lines.append(f'    mut {f}{i} = bodies[{i}].{f}')
    lines += ['    dtv = simd.f64x2.Lanes(dt, dt)', '    for _ in range(steps):']
    seen = set()
    def bind(lhs, rhs):
        prefix = 'mut ' if lhs not in seen else ''
        seen.add(lhs)
        lines.append(f'        {prefix}{lhs} = {rhs}')
    for i, j in PAIRS:
        bind('dxy', f'p{i}.sub(&p{j})')
        bind('dz', f'z{i} - z{j}')
        bind('sx, sy', 'dxy.mul(&dxy).to_tuple()')
        bind('d2', 'sx + sy + dz * dz')
        bind('mag', 'dt / (d2 * d2.sqrt())')
        bind('mi', f'mass{i} * mag')
        bind('mj', f'mass{j} * mag')
        lines.append(f'        v{i} = v{i}.sub(&dxy.mul(&simd.f64x2.Lanes(mj, mj)))')
        lines.append(f'        vz{i} -= dz * mj')
        lines.append(f'        v{j} = v{j}.add(&dxy.mul(&simd.f64x2.Lanes(mi, mi)))')
        lines.append(f'        vz{j} += dz * mi')
    for i in range(5):
        lines += [f'        p{i} = p{i}.add(&v{i}.mul(&dtv))', f'        z{i} += dt * vz{i}']
    for i in range(5):
        for v, fields in [('p', ('x', 'y')), ('v', ('vx', 'vy'))]:
            a, b = fields
            lines += [f'    {a}{i}, {b}{i} = {v}{i}.to_tuple()',
                      f'    bodies[{i}].{a} = {a}{i}', f'    bodies[{i}].{b} = {b}{i}']
        for f in ('z', 'vz'):
            lines.append(f'    bodies[{i}].{f} = {f}{i}')
    lines.append('    Ok(())')
    (out / 'simd-xy.plenty').write_text('import std.simd as simd\n' + source[:start] +
        '\n'.join(lines) + '\n' + source[end:])


def c_locals(out):
    p = (out / 'locals.plenty').read_text()
    s = p[p.index('    mut x0 ='):p.index('\n\ntype AppError')]
    s = s.replace('    for _ in range(steps):', '    for (int64_t t=0; t<steps; ++t) {')
    s = s.replace('    Ok(())', '}')
    s = re.sub(r'bodies\[(\d)\]\.([a-z]+)', r'b[\1].\2', s)
    s = re.sub(r'(\s*)mut (\w+) =', r'\1double \2 =', s)
    s = s.replace('d2.sqrt()', 'sqrt(d2)')
    lines = []
    for line in s.splitlines():
        if line.startswith('    b[0].x ='):
            lines.append('    }')
        lines.append(line + (';' if line.strip() and not line.rstrip().endswith(('{', '}')) else ''))
    (out / 'locals.c').write_text('#include <math.h>\n#include <stdint.h>\n'
        'typedef struct {double x,y,z,vx,vy,vz,mass;} Body;\n'
        'void locals(Body *b, int64_t steps) {\n    const double dt=0.01;\n' +
        '\n'.join(lines) + '\n')


def snapshots(out):
    (out / 'original.plenty').write_text(
        (ROOT / 'benchmarks/game/n-body/plenty/nbody.plenty').read_text())
    for name in ('original', 'fused', 'locals', 'simd-locals', 'simd-xy'):
        path = out / f'{name}.plenty'
        source = path.read_text()
        start = source.rindex('    Ok(())')
        fields = ' '.join('{b.' + f + ':.17e}' for f in FIELDS)
        source = source[:start] + ('    if len(argv) > 2:\n'
            '        for b in &bodies:\n'
            f'            print(f"{fields}"?)?\n') + source[start:]
        path.write_text(source)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('output', type=Path)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    plenty(args.output)
    simd(args.output)
    simd_xy(args.output)
    c_locals(args.output)
    snapshots(args.output)


if __name__ == '__main__':
    main()
