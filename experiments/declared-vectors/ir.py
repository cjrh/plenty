#!/usr/bin/env python3
"""Bounded native SSA probes; no public ABI or compiler lowering is changed.

recurrence: void fn(double state[3], const double delta[3], double scale, i64 n)
nbody: void fn(double state[35], i64 steps), seven doubles per body.
Pointers must cover the indicated logical data for positive step counts.
All loads/stores use three logical components; native padding is constructed.
"""
import argparse
import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('nbody_ir', ROOT / 'experiments/nbody-structural/ir.py')
old = importlib.util.module_from_spec(spec)
spec.loader.exec_module(old)


class IR(old.IR):
    def __init__(self, shape):
        super().__init__()
        self.shape = shape
        self.types = ['f64'] * 3 if shape == 'scalar' else ['f64x2'] * 2 if shape == 'halves' else ['f64x4']

    def vector(self, values):
        zero = self.emit('f64const 0x0.0p0')
        if self.shape == 'scalar':
            return values
        if self.shape == 'halves':
            low = self.emit(f'splat.f64x2 {values[0]}')
            low = self.emit(f'insertlane {low}, {values[1]}, 1')
            high = self.emit(f'splat.f64x2 {zero}')
            high = self.emit(f'insertlane {high}, {values[2]}, 0')
            return [low, high]
        vector = self.emit(f'splat.f64x4 {zero}')
        for lane, value in enumerate(values):
            vector = self.emit(f'insertlane {vector}, {value}, {lane}')
        return [vector]

    def multiplier(self, scalar):
        if self.shape == 'scalar':
            return [scalar] * 3
        one = self.emit('f64const 0x1.0p0')
        if self.shape == 'halves':
            low = self.emit(f'splat.f64x2 {scalar}')
            high = self.emit(f'splat.f64x2 {one}')
            high = self.emit(f'insertlane {high}, {scalar}, 0')
            return [low, high]
        vector = self.emit(f'splat.f64x4 {scalar}')
        vector = self.emit(f'insertlane {vector}, {one}, 3')
        return [vector]

    def logical(self, parts):
        if self.shape == 'scalar':
            return parts
        if self.shape == 'halves':
            return [self.emit(f'extractlane {parts[0]}, 0'),
                    self.emit(f'extractlane {parts[0]}, 1'),
                    self.emit(f'extractlane {parts[1]}, 0')]
        return [self.emit(f'extractlane {parts[0]}, {lane}') for lane in range(3)]

    def op(self, name, left, right):
        return [self.binary(name, a, b) for a, b in zip(left, right)]

    def parameters(self, count):
        values = []
        for _ in range(count):
            values.append(f'v{self.index}')
            self.index += 1
        return values

    def recurrence(self):
        self.index = 4
        self.lines = [f'function %recurrence_{self.shape}(i64, i64, f64, i64) system_v {{',
                      'block0(v0: i64, v1: i64, v2: f64, v3: i64):']
        zero = self.emit('iconst.i64 0')
        positive = self.emit(f'icmp sgt v3, {zero}')
        self.lines += [f'    brif {positive}, block1, block4', 'block1:']
        state = self.vector([self.emit(f'load.f64 v0+{i * 8}') for i in range(3)])
        delta = self.vector([self.emit(f'load.f64 v1+{i * 8}') for i in range(3)])
        scale = self.multiplier('v2')
        self.lines.append(f'    jump block2(v3, {", ".join(state)})')
        n, *values = self.parameters(1 + len(state))
        params = [f'{n}: i64'] + [f'{v}: {t}' for v, t in zip(values, self.types)]
        self.lines.append('block2(' + ', '.join(params) + '):')
        final = self.op('fadd', self.op('fmul', values, scale), delta)
        one = self.emit('iconst.i64 1')
        remaining = self.binary('isub', n, one)
        positive = self.emit(f'icmp sgt {remaining}, {zero}')
        self.lines += [f'    brif {positive}, block2({remaining}, {", ".join(final)}), block3', 'block3:']
        for lane, value in enumerate(self.logical(final)):
            self.lines.append(f'    store {value}, v0+{lane * 8}')
        self.lines += ['    jump block4', 'block4:', '    return', '}']
        return '\n'.join(self.lines) + '\n'

    def nbody(self):
        self.index = 2
        self.lines = [f'function %advance_{self.shape}(i64, i64) system_v {{', 'block0(v0: i64, v1: i64):']
        zero = self.emit('iconst.i64 0')
        positive = self.emit(f'icmp sgt v1, {zero}')
        self.lines += [f'    brif {positive}, block1, block4', 'block1:']
        dt = self.emit('f64const ' + (0.01).hex())
        dtv = self.multiplier(dt)
        state, masses = [], []
        for body in range(5):
            for field in (0, 3):
                state += self.vector([self.emit(f'load.f64 v0+{body * 56 + (field + lane) * 8}') for lane in range(3)])
            masses.append(self.emit(f'load.f64 v0+{body * 56 + 48}'))
        self.lines.append(f'    jump block2(v1, {", ".join(state)})')
        n, *params = self.parameters(1 + len(state))
        types = self.types * 10
        self.lines.append('block2(' + ', '.join([f'{n}: i64'] + [f'{v}: {t}' for v, t in zip(params, types)]) + '):')
        width = len(self.types)
        positions = [params[2 * width * i:(2 * i + 1) * width] for i in range(5)]
        velocities = [params[(2 * i + 1) * width:(2 * i + 2) * width] for i in range(5)]
        for i, j in old.PAIRS:
            difference = self.op('fsub', positions[i], positions[j])
            square = self.logical(self.op('fmul', difference, difference))
            squared = self.binary('fadd', self.binary('fadd', square[0], square[1]), square[2])
            root = self.emit(f'sqrt {squared}')
            magnitude = self.binary('fdiv', dt, self.binary('fmul', squared, root))
            mi = self.multiplier(self.binary('fmul', masses[i], magnitude))
            mj = self.multiplier(self.binary('fmul', masses[j], magnitude))
            velocities[i] = self.op('fsub', velocities[i], self.op('fmul', difference, mj))
            velocities[j] = self.op('fadd', velocities[j], self.op('fmul', difference, mi))
        for i in range(5):
            positions[i] = self.op('fadd', positions[i], self.op('fmul', velocities[i], dtv))
        final = [value for i in range(5) for group in (positions[i], velocities[i]) for value in group]
        one = self.emit('iconst.i64 1')
        remaining = self.binary('isub', n, one)
        positive = self.emit(f'icmp sgt {remaining}, {zero}')
        self.lines += [f'    brif {positive}, block2({remaining}, {", ".join(final)}), block3', 'block3:']
        for i in range(5):
            for field, group in [(0, positions[i]), (3, velocities[i])]:
                for lane, value in enumerate(self.logical(group)):
                    self.lines.append(f'    store {value}, v0+{i * 56 + (field + lane) * 8}')
        self.lines += ['    jump block4', 'block4:', '    return', '}']
        return '\n'.join(self.lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for shape in ['scalar', 'halves', 'wide']:
        (args.output / (shape + '.clif')).write_text(IR(shape).recurrence())
        (args.output / ('nbody-' + shape + '.clif')).write_text(IR(shape).nbody())


if __name__ == '__main__':
    main()
