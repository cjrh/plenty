// Isolated n-body magnitude phase; same operation order, no fast-math.
#include <math.h>
#include <stddef.h>
void magnitude(const double *x, const double *y, const double *z,
               double *out, size_t n, double dt) {
    for (size_t i = 0; i < n; ++i) {
        double d2 = x[i]*x[i] + y[i]*y[i] + z[i]*z[i];
        out[i] = dt / (d2 * sqrt(d2));
    }
}
