#!/usr/bin/env python3
"""Fixed-five-body Cranelift kernels that pack only the expensive operations.

Same ABI, state layout and operation order as
`experiments/nbody-structural/ir.py`'s scalar kernel. Variants:

  scalar        every operation scalar (the structural study's kernel)
  pack-tree     per two pairs: d2 packed once, then sqrt, multiply and divide
                as f64x2; one lane extraction per pair
  pack-stmt     per two pairs: the differences packed, then every operation of
                both magnitude statements as f64x2; two lane extractions
  pack-ops      per two pairs: sqrt packed and split, scalar multiply, divide
                packed and split (what packing isolated operations gives)
"""
from pathlib import Path
import argparse

MODES = ('scalar', 'pack-tree', 'pack-stmt', 'pack-ops')
PAIRS = [(i, j) for i in range(5) for j in range(i + 1, 5)]


class IR:
    def __init__(self):
        self.lines, self.index = [], 2

    def emit(self, op):
        v = f'v{self.index}'
        self.index += 1
        self.lines.append(f'    {v} = {op}')
        return v

    def binary(self, op, a, b):
        return self.emit(f'{op} {a}, {b}')

    def pack(self, a, b):
        v = self.emit(f'scalar_to_vector.f64x2 {a}')
        return self.emit(f'insertlane {v}, {b}, 1')

    def split(self, v):
        return [self.emit(f'extractlane {v}, {lane}') for lane in range(2)]

    def generate(self, mode):
        self.lines = [f'function %advance_{mode.replace("-", "_")}(i64, i64) system_v {{', 'block0(v0: i64, v1: i64):']
        zero = self.emit('iconst.i64 0')
        positive = self.emit(f'icmp sgt v1, {zero}')
        self.lines += [f'    brif {positive}, block1, block4', 'block1:']
        dt = self.emit('f64const ' + (0.01).hex())
        dtv = self.emit(f'splat.f64x2 {dt}')
        state, offsets, masses = [], [], []
        for i in range(5):
            for f in range(6):
                offsets.append(i * 56 + f * 8)
                state.append(self.emit(f'load.f64 v0+{offsets[-1]}'))
            masses.append(self.emit(f'load.f64 v0+{i * 56 + 48}'))
        self.lines.append(f'    jump block2(v1, {", ".join(state)})')
        n = f'v{self.index}'
        self.index += 1
        params = []
        for _ in state:
            params.append(f'v{self.index}')
            self.index += 1
        self.lines.append(f'block2({n}: i64, ' + ', '.join(f'{v}: f64' for v in params) + '):')
        values = [params[i * 6:(i + 1) * 6] for i in range(5)]

        def distance(i, j):
            d = [self.binary('fsub', values[i][f], values[j][f]) for f in range(3)]
            sq = [self.binary('fmul', v, v) for v in d]
            return d, self.binary('fadd', self.binary('fadd', sq[0], sq[1]), sq[2])

        batch = 1 if mode == 'scalar' else 2
        for k in range(0, 10, batch):
            pairs = PAIRS[k:k + batch]
            distances = [distance(i, j) for i, j in pairs]
            if mode == 'scalar':
                d2 = distances[0][1]
                mags = [self.binary('fdiv', dt, self.binary('fmul', d2, self.emit(f'sqrt {d2}')))]
            elif mode == 'pack-tree':
                d2v = self.pack(distances[0][1], distances[1][1])
                mags = self.split(self.binary('fdiv', dtv, self.binary('fmul', d2v, self.emit(f'sqrt {d2v}'))))
            elif mode == 'pack-stmt':
                dv = [self.pack(distances[0][0][f], distances[1][0][f]) for f in range(3)]
                sq = [self.binary('fmul', v, v) for v in dv]
                d2v = self.binary('fadd', self.binary('fadd', sq[0], sq[1]), sq[2])
                mags = self.split(self.binary('fdiv', dtv, self.binary('fmul', d2v, self.emit(f'sqrt {d2v}'))))
            else:
                roots = self.split(self.emit(f'sqrt {self.pack(distances[0][1], distances[1][1])}'))
                cubes = [self.binary('fmul', d[1], r) for d, r in zip(distances, roots)]
                mags = self.split(self.binary('fdiv', dtv, self.pack(*cubes)))
            for (i, j), (d, _), mag in zip(pairs, distances, mags):
                mi = self.binary('fmul', masses[i], mag)
                mj = self.binary('fmul', masses[j], mag)
                for f in range(3):
                    values[i][f + 3] = self.binary('fsub', values[i][f + 3], self.binary('fmul', d[f], mj))
                    values[j][f + 3] = self.binary('fadd', values[j][f + 3], self.binary('fmul', d[f], mi))
        for body in values:
            for f in range(3):
                body[f] = self.binary('fadd', body[f], self.binary('fmul', dt, body[f + 3]))
        final = [v for body in values for v in body]
        rem = self.binary('isub', n, self.emit('iconst.i64 1'))
        cond = self.emit(f'icmp sgt {rem}, {zero}')
        self.lines += [f'    brif {cond}, block2({rem}, {", ".join(final)}), block3', 'block3:']
        self.lines += [f'    store {v}, v0+{offset}' for v, offset in zip(final, offsets)]
        self.lines += ['    jump block4', 'block4:', '    return', '}']
        return '\n'.join(self.lines) + '\n'


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('output', type=Path)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    for mode in MODES:
        (args.output / f'{mode}.clif').write_text(IR().generate(mode))


if __name__ == '__main__':
    main()
