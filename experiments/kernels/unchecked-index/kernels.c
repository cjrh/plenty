#include <stdint.h>
double gather(const double *xs, const int64_t *indices, int64_t n) {
    double total = 0.0;
    for (int64_t i = 0; i < n; ++i) total += xs[indices[i]];
    return total;
}
