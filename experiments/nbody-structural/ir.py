#!/usr/bin/env python3
"""Emit complete, executable fixed-five-body Cranelift experiments.

C ABI: void advance(double *35_element_state, int64_t steps).
The state layout is 7 doubles/body. Loads/stores are outside the timestep loop.
No assumptions about the floating values; pair/update order is unchanged.
"""
from pathlib import Path
import argparse

PAIRS = [(i, j) for i in range(5) for j in range(i + 1, 5)]

class IR:
    def __init__(self):
        self.lines = []
        self.index = 2

    def emit(self, op):
        v = f'v{self.index}'
        self.index += 1
        self.lines.append(f'    {v} = {op}')
        return v

    def binary(self, op, a, b):
        return self.emit(f'{op} {a}, {b}')

    def generate(self, mode):
        xy = mode.startswith('xy')
        self.lines = ['function %advance_' + mode.replace('-', '_') + '(i64, i64) system_v {', 'block0(v0: i64, v1: i64):']
        zero = self.emit('iconst.i64 0')
        positive = self.emit(f'icmp sgt v1, {zero}')
        self.lines.append(f'    brif {positive}, block1, block4')
        self.lines.append('block1:')
        dt = self.emit('f64const ' + (0.01).hex())
        dtv = self.emit(f'splat.f64x2 {dt}') if xy else None
        state, types, offsets = [], [], []
        masses = []
        for i in range(5):
            for f, ty in ([(0, 'f64x2'), (2, 'f64'), (3, 'f64x2'), (5, 'f64')] if xy else [(f, 'f64') for f in range(6)]):
                offset = i * 56 + f * 8
                state.append(self.emit(f'load.{ty} v0+{offset}'))
                types.append(ty)
                offsets.append(offset)
            masses.append(self.emit(f'load.f64 v0+{i * 56 + 48}'))
        self.lines.append(f'    jump block2(v1, {", ".join(state)})')
        n = f'v{self.index}'; self.index += 1
        params = []
        for _ in state:
            params.append(f'v{self.index}'); self.index += 1
        self.lines.append(f'block2({n}: i64, ' + ', '.join(f'{v}: {t}' for v, t in zip(params, types)) + '):')
        width = 4 if xy else 6
        values = [params[i*width:(i+1)*width] for i in range(5)]
        dims = 2 if xy else 3
        def distance(i, j):
            d = [self.binary('fsub', values[i][f], values[j][f]) for f in range(dims)]
            square = [self.binary('fmul', v, v) for v in d]
            if xy:
                sx = self.emit(f'extractlane {square[0]}, 0')
                sy = self.emit(f'extractlane {square[0]}, 1')
                d2 = self.binary('fadd', self.binary('fadd', sx, sy), square[1])
            else:
                d2 = self.binary('fadd', self.binary('fadd', square[0], square[1]), square[2])
            return d, d2

        batch_size = 2 if mode.endswith('pairs') else 1
        for k in range(0, 10, batch_size):
            pairs = PAIRS[k:k+batch_size]
            distances = [distance(i, j) for i, j in pairs]
            if batch_size == 2:
                d2v = self.emit(f'splat.f64x2 {distances[0][1]}')
                d2v = self.emit(f'insertlane {d2v}, {distances[1][1]}, 1')
                root = self.emit(f'sqrt {d2v}')
                magv = self.binary('fdiv', dtv, self.binary('fmul', d2v, root))
                mags = [self.emit(f'extractlane {magv}, {lane}') for lane in range(2)]
            else:
                d2 = distances[0][1]
                root = self.emit(f'sqrt {d2}')
                mags = [self.binary('fdiv', dt, self.binary('fmul', d2, root))]
            for (i, j), (d, _), mag in zip(pairs, distances, mags):
                mi = self.binary('fmul', masses[i], mag)
                mj = self.binary('fmul', masses[j], mag)
                miv = self.emit(f'splat.f64x2 {mi}') if xy else mi
                mjv = self.emit(f'splat.f64x2 {mj}') if xy else mj
                for f in range(dims):
                    vi = values[i][f+dims]; vj = values[j][f+dims]
                    values[i][f+dims] = self.binary('fsub', vi, self.binary('fmul', d[f], mjv if f == 0 else mj))
                    values[j][f+dims] = self.binary('fadd', vj, self.binary('fmul', d[f], miv if f == 0 else mi))
        dims = 2 if xy else 3
        for body in values:
            for f in range(dims):
                body[f] = self.binary('fadd', body[f], self.binary('fmul', dtv if xy and f == 0 else dt, body[f+dims]))
        final = [v for body in values for v in body]
        one = self.emit('iconst.i64 1')
        rem = self.binary('isub', n, one)
        cond = self.emit(f'icmp sgt {rem}, {zero}')
        self.lines.append(f'    brif {cond}, block2({rem}, {", ".join(final)}), block3')
        self.lines.append('block3:')
        for v, offset in zip(final, offsets):
            self.lines.append(f'    store {v}, v0+{offset}')
        self.lines += ['    jump block4', 'block4:', '    return', '}']
        return '\n'.join(self.lines) + '\n'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('output', type=Path)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for mode in ('scalar', 'xy', 'xy-pairs'):
        (args.output / f'{mode}.clif').write_text(IR().generate(mode))

if __name__ == '__main__':
    main()
