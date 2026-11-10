// Same pair/update order as the benchmark. No restrict or fast-math.
#include <stddef.h>
typedef struct { double x, y, z, vx, vy, vz, mass; } Body;
void velocity(Body *b, const double *dx, const double *dy,
              const double *dz, const double *mag, size_t n) {
    size_t k = 0;
    for (size_t i = 0; i < n; ++i) {
        for (size_t j = i + 1; j < n; ++j) {
            double mi = b[i].mass * mag[k];
            double mj = b[j].mass * mag[k];
            b[i].vx -= dx[k] * mj;
            b[i].vy -= dy[k] * mj;
            b[i].vz -= dz[k] * mj;
            b[j].vx += dx[k] * mi;
            b[j].vy += dy[k] * mi;
            b[j].vz += dz[k] * mi;
            ++k;
        }
    }
}
