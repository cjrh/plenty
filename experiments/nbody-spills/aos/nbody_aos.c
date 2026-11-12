/* n-body with a heap array of structs whose length is known only at run
   time: the C counterpart of nbody_clist.plenty and nbody.plenty. Same
   statement order as the Plenty sources. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>

#define PI 3.141592653589793
#define SOLAR_MASS (4.0 * PI * PI)
#define DAYS 365.24

struct body { double x, y, z, vx, vy, vz, mass; };

static const struct body START[] = {
    {0, 0, 0, 0, 0, 0, SOLAR_MASS},
    {4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01,
     1.66007664274403694e-03 * DAYS, 7.69901118419740425e-03 * DAYS, -6.90460016972063023e-05 * DAYS,
     9.54791938424326609e-04 * SOLAR_MASS},
    {8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01,
     -2.76742510726862411e-03 * DAYS, 4.99852801234917238e-03 * DAYS, 2.30417297573763929e-05 * DAYS,
     2.85885980666130812e-04 * SOLAR_MASS},
    {1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01,
     2.96460137564761618e-03 * DAYS, 2.37847173959480950e-03 * DAYS, -2.96589568540237556e-05 * DAYS,
     4.36624404335156298e-05 * SOLAR_MASS},
    {1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01,
     2.68067772490389322e-03 * DAYS, 1.62824170038242295e-03 * DAYS, -9.51592254519715870e-05 * DAYS,
     5.15138902046611451e-05 * SOLAR_MASS},
};

__attribute__((noinline)) static void offset_momentum(struct body *b, long n) {
    double px = 0, py = 0, pz = 0;
    for (long i = 0; i < n; i++) {
        px += b[i].vx * b[i].mass;
        py += b[i].vy * b[i].mass;
        pz += b[i].vz * b[i].mass;
    }
    b[0].vx = -px / SOLAR_MASS;
    b[0].vy = -py / SOLAR_MASS;
    b[0].vz = -pz / SOLAR_MASS;
}

__attribute__((noinline)) static double energy(const struct body *b, long n) {
    double e = 0;
    for (long i = 0; i < n; i++) {
        e += 0.5 * b[i].mass * (b[i].vx * b[i].vx + b[i].vy * b[i].vy + b[i].vz * b[i].vz);
        for (long j = i + 1; j < n; j++) {
            double dx = b[i].x - b[j].x, dy = b[i].y - b[j].y, dz = b[i].z - b[j].z;
            e -= b[i].mass * b[j].mass / sqrt(dx * dx + dy * dy + dz * dz);
        }
    }
    return e;
}

__attribute__((noinline)) static int advance(struct body *b, long n, double dt, long steps) {
    long pairs = n * (n - 1) / 2;
    double *dx = calloc(pairs, sizeof *dx), *dy = calloc(pairs, sizeof *dy);
    double *dz = calloc(pairs, sizeof *dz), *mag = calloc(pairs, sizeof *mag);
    if (!dx || !dy || !dz || !mag) return 1;
    for (long s = 0; s < steps; s++) {
        long k = 0;
        for (long i = 0; i < n; i++)
            for (long j = i + 1; j < n; j++) {
                dx[k] = b[i].x - b[j].x;
                dy[k] = b[i].y - b[j].y;
                dz[k] = b[i].z - b[j].z;
                k++;
            }
        for (long p = 0; p < pairs; p++) {
            double d2 = dx[p] * dx[p] + dy[p] * dy[p] + dz[p] * dz[p];
            mag[p] = dt / (d2 * sqrt(d2));
        }
        k = 0;
        for (long i = 0; i < n; i++)
            for (long j = i + 1; j < n; j++) {
                double mi = b[i].mass * mag[k], mj = b[j].mass * mag[k];
                b[i].vx -= dx[k] * mj;
                b[i].vy -= dy[k] * mj;
                b[i].vz -= dz[k] * mj;
                b[j].vx += dx[k] * mi;
                b[j].vy += dy[k] * mi;
                b[j].vz += dz[k] * mi;
                k++;
            }
        for (long i = 0; i < n; i++) {
            b[i].x += dt * b[i].vx;
            b[i].y += dt * b[i].vy;
            b[i].z += dt * b[i].vz;
        }
    }
    free(dx); free(dy); free(dz); free(mag);
    return 0;
}

int main(int argc, char **argv) {
    long steps = argc > 1 ? atol(argv[1]) : 1000;
    /* Opaque, so that no pass specializes `advance` for five bodies. */
    volatile long count = sizeof START / sizeof *START;
    long n = count;
    struct body *b = malloc(n * sizeof *b);
    if (!b) return 1;
    for (long i = 0; i < n; i++) b[i] = START[i];
    offset_momentum(b, n);
    printf("%.9f\n", energy(b, n));
    if (advance(b, n, 0.01, steps)) return 1;
    printf("%.9f\n", energy(b, n));
    free(b);
    return 0;
}
