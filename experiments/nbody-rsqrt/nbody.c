// n-body with the magnitude step of the fast Benchmarks Game C entries:
// rsqrtps on the narrowed squared distances, two Newton-Raphson steps in
// double precision. Build with -DEXACT for sqrtpd and divpd instead.
// Same step structure as benchmarks/game/n-body/plenty/nbody.plenty.
#include <immintrin.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>

#define N 5
#define PAIRS (N * (N - 1) / 2)
#define PI 3.141592653589793
#define SOLAR_MASS (4 * PI * PI)
#define DAYS 365.24

struct body { double x, y, z, vx, vy, vz, mass; };

static struct body bodies[N] = {
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

static void offset_momentum(void) {
    double px = 0, py = 0, pz = 0;
    for (int i = 0; i < N; i++) {
        px += bodies[i].vx * bodies[i].mass;
        py += bodies[i].vy * bodies[i].mass;
        pz += bodies[i].vz * bodies[i].mass;
    }
    bodies[0].vx = -px / SOLAR_MASS;
    bodies[0].vy = -py / SOLAR_MASS;
    bodies[0].vz = -pz / SOLAR_MASS;
}

static double energy(void) {
    double e = 0;
    for (int i = 0; i < N; i++) {
        struct body *b = &bodies[i];
        e += 0.5 * b->mass * (b->vx * b->vx + b->vy * b->vy + b->vz * b->vz);
        for (int j = i + 1; j < N; j++) {
            double dx = b->x - bodies[j].x, dy = b->y - bodies[j].y, dz = b->z - bodies[j].z;
            e -= b->mass * bodies[j].mass / sqrt(dx * dx + dy * dy + dz * dz);
        }
    }
    return e;
}

static void advance(double dt, long steps) {
    static double dx[PAIRS] __attribute__((aligned(16)));
    static double dy[PAIRS] __attribute__((aligned(16)));
    static double dz[PAIRS] __attribute__((aligned(16)));
    static double mag[PAIRS] __attribute__((aligned(16)));
    for (long s = 0; s < steps; s++) {
        int k = 0;
        for (int i = 0; i < N; i++)
            for (int j = i + 1; j < N; j++, k++) {
                dx[k] = bodies[i].x - bodies[j].x;
                dy[k] = bodies[i].y - bodies[j].y;
                dz[k] = bodies[i].z - bodies[j].z;
            }
        for (int p = 0; p < PAIRS; p += 2) {
            __m128d a = _mm_load_pd(dx + p), b = _mm_load_pd(dy + p), c = _mm_load_pd(dz + p);
            __m128d d2 = _mm_add_pd(_mm_add_pd(_mm_mul_pd(a, a), _mm_mul_pd(b, b)), _mm_mul_pd(c, c));
#ifdef EXACT
            __m128d m = _mm_div_pd(_mm_set1_pd(dt), _mm_mul_pd(d2, _mm_sqrt_pd(d2)));
#else
            __m128d y = _mm_cvtps_pd(_mm_rsqrt_ps(_mm_cvtpd_ps(d2)));
            __m128d half = _mm_mul_pd(d2, _mm_set1_pd(0.5));
            for (int r = 0; r < 2; r++)
                y = _mm_sub_pd(_mm_mul_pd(y, _mm_set1_pd(1.5)),
                               _mm_mul_pd(_mm_mul_pd(half, y), _mm_mul_pd(y, y)));
            __m128d m = _mm_mul_pd(_mm_div_pd(_mm_set1_pd(dt), d2), y);
#endif
            _mm_store_pd(mag + p, m);
        }
        k = 0;
        for (int i = 0; i < N; i++)
            for (int j = i + 1; j < N; j++, k++) {
                double mi = bodies[i].mass * mag[k], mj = bodies[j].mass * mag[k];
                bodies[i].vx -= dx[k] * mj;
                bodies[i].vy -= dy[k] * mj;
                bodies[i].vz -= dz[k] * mj;
                bodies[j].vx += dx[k] * mi;
                bodies[j].vy += dy[k] * mi;
                bodies[j].vz += dz[k] * mi;
            }
        for (int i = 0; i < N; i++) {
            bodies[i].x += dt * bodies[i].vx;
            bodies[i].y += dt * bodies[i].vy;
            bodies[i].z += dt * bodies[i].vz;
        }
    }
}

int main(int argc, char **argv) {
    long steps = argc > 1 ? atol(argv[1]) : 1000;
    offset_momentum();
    printf("%.9f\n", energy());
    advance(0.01, steps);
    printf("%.9f\n", energy());
    return 0;
}
