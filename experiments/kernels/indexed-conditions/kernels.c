/* C counterparts of kernels.plenty, without bounds checks. */
#include <stdint.h>

int64_t find(const int64_t *limits, int64_t r) {
    int64_t k = 0;
    while (limits[k] < r)
        k++;
    return k;
}

void select_all(const int64_t *limits, const int64_t *targets, int64_t n, int64_t *out) {
    for (int64_t i = 0; i < n; i++) {
        int64_t k = 0;
        while (limits[k] < targets[i])
            k++;
        out[i] = k;
    }
}

int64_t total(const int64_t *values, int64_t n) {
    int64_t t = 0;
    for (int64_t i = 0; i < n; i++)
        t += values[i];
    return t;
}
