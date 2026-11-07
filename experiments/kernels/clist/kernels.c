/* Issue #759: the loops of kernels.plenty over plain column arrays. */
#include <math.h>

struct bodies {
    double *x, *y, *z, *vx, *vy, *vz, *mass;
    long len;
};

long velocity(struct bodies *b, const double *dx, const double *dy,
              const double *dz, const double *mag) {
    long n = b->len, k = 0;
    for (long i = 0; i < n; i++) {
        for (long j = i + 1; j < n; j++) {
            double mi = b->mass[i] * mag[k];
            double mj = b->mass[j] * mag[k];
            b->vx[i] -= dx[k] * mj;
            b->vy[i] -= dy[k] * mj;
            b->vz[i] -= dz[k] * mj;
            b->vx[j] += dx[k] * mi;
            b->vy[j] += dy[k] * mi;
            b->vz[j] += dz[k] * mi;
            k++;
        }
    }
    return k;
}

void positions(struct bodies *b, double dt) {
    for (long i = 0; i < b->len; i++) {
        b->x[i] += dt * b->vx[i];
        b->y[i] += dt * b->vy[i];
        b->z[i] += dt * b->vz[i];
    }
}

double potential(const struct bodies *b) {
    double e = 0.0;
    long n = b->len;
    for (long i = 0; i < n; i++) {
        for (long j = i + 1; j < n; j++) {
            double dx = b->x[i] - b->x[j];
            double dy = b->y[i] - b->y[j];
            double dz = b->z[i] - b->z[j];
            e -= b->mass[i] * b->mass[j] / sqrt(dx * dx + dy * dy + dz * dz);
        }
    }
    return e;
}
