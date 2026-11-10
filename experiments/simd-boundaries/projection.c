/* Same recurrence and FP order as projection.plenty; baseline SSE2. */
#include <stdint.h>
#include <emmintrin.h>
double projection(double a, double b, int64_t n) {
    __m128d v = _mm_set_pd(b, a);
    const __m128d delta = _mm_set_pd(-0.0625, 0.125);
    double total = 0.0;
    for (int64_t i = 0; i < n; ++i) {
        v = _mm_add_pd(v, delta);
        double x = _mm_cvtsd_f64(v);
        double y = _mm_cvtsd_f64(_mm_unpackhi_pd(v, v));
        total += x * y;
    }
    return total;
}
