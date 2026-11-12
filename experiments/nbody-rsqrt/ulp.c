// Max ulp error of the n-body magnitude dt/d2 * y (y: rsqrtps + two NR steps in f64)
// against dt/(d2*sqrt(d2)), over d2 in the range n-body sees and a wide sweep.
#include <immintrin.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
static int64_t ord(double x) { int64_t i; memcpy(&i, &x, 8); return i < 0 ? INT64_MIN - i : i; }
static double est(double d2) {
    __m128 f = _mm_set_ss((float)d2);
    double y = (double)_mm_cvtss_f32(_mm_rsqrt_ps(f));
    double h = 0.5 * d2;
    y = y * 1.5 - h * y * (y * y);
    y = y * 1.5 - h * y * (y * y);
    return y;
}
int main(void) {
    double lo[] = {1e-2, 1e-30}, hi[] = {1e3, 1e30};
    for (int r = 0; r < 2; r++) {
        int64_t worst_y = 0, worst_m = 0; double at = 0;
        double ratio = pow(hi[r] / lo[r], 1.0 / 2e7);
        for (double d2 = lo[r]; d2 < hi[r]; d2 *= ratio) {
            double y = est(d2), exact_y = 1.0 / sqrt(d2);
            double m = 0.01 / d2 * y, exact_m = 0.01 / (d2 * sqrt(d2));
            int64_t ey = llabs(ord(y) - ord(exact_y)), em = llabs(ord(m) - ord(exact_m));
            if (ey > worst_y) worst_y = ey;
            if (em > worst_m) { worst_m = em; at = d2; }
        }
        printf("d2 in [%g, %g]: max ulp 1/sqrt %lld, magnitude %lld (at %g)\n", lo[r], hi[r], (long long)worst_y, (long long)worst_m, at);
    }
}
