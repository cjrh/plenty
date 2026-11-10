/* Diagnostic kernels adapted from the repository's Benchmarks Game port.
 * Same pair order and scalar operation order; no fast-math or restrict.
 * Body layout is deliberately ordinary packed doubles, unlike Plenty's rows.
 */
#include <math.h>
#include <stdint.h>

typedef struct { double x, y, z, vx, vy, vz, mass; } Body;

void staged(Body *b, int64_t steps) {
    double dx[10], dy[10], dz[10], mag[10];
    for (int64_t t = 0; t < steps; ++t) {
        int k = 0;
        for (int i = 0; i < 5; ++i)
            for (int j = i + 1; j < 5; ++j, ++k) {
                dx[k] = b[i].x - b[j].x;
                dy[k] = b[i].y - b[j].y;
                dz[k] = b[i].z - b[j].z;
            }
        for (int p = 0; p < 10; ++p) {
            double d2 = dx[p]*dx[p] + dy[p]*dy[p] + dz[p]*dz[p];
            mag[p] = 0.01 / (d2 * sqrt(d2));
        }
        k = 0;
        for (int i = 0; i < 5; ++i)
            for (int j = i + 1; j < 5; ++j, ++k) {
                double mi = b[i].mass * mag[k], mj = b[j].mass * mag[k];
                b[i].vx -= dx[k]*mj; b[i].vy -= dy[k]*mj; b[i].vz -= dz[k]*mj;
                b[j].vx += dx[k]*mi; b[j].vy += dy[k]*mi; b[j].vz += dz[k]*mi;
            }
        for (int i = 0; i < 5; ++i) {
            b[i].x += 0.01*b[i].vx;
            b[i].y += 0.01*b[i].vy;
            b[i].z += 0.01*b[i].vz;
        }
    }
}

void fused(Body *b, int64_t steps) {
    for (int64_t t = 0; t < steps; ++t) {
        for (int i = 0; i < 5; ++i)
            for (int j = i + 1; j < 5; ++j) {
                double dx = b[i].x - b[j].x;
                double dy = b[i].y - b[j].y;
                double dz = b[i].z - b[j].z;
                double d2 = dx*dx + dy*dy + dz*dz;
                double mag = 0.01 / (d2 * sqrt(d2));
                double mi = b[i].mass * mag, mj = b[j].mass * mag;
                b[i].vx -= dx*mj; b[i].vy -= dy*mj; b[i].vz -= dz*mj;
                b[j].vx += dx*mi; b[j].vy += dy*mi; b[j].vz += dz*mi;
            }
        for (int i = 0; i < 5; ++i) {
            b[i].x += 0.01*b[i].vx;
            b[i].y += 0.01*b[i].vy;
            b[i].z += 0.01*b[i].vz;
        }
    }
}
