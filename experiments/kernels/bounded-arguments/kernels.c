#include <stdint.h>
void a_times_u(const double *u, double *out, int64_t n) {
    if (n > 1000000) return;
    for (int64_t i = 0; i < n; ++i) {
        double total = 0;
        for (int64_t j = 0; j < n; ++j) {
            int64_t ij = i + j;
            total += (1.0 / (double)(ij * (ij + 1) / 2 + i + 1)) * u[j];
        }
        out[i] = total;
    }
}

void at_times_u(const double *u, double *out, int64_t n) {
    if (n > 1000000) return;
    for (int64_t i = 0; i < n; ++i) {
        double total = 0;
        for (int64_t j = 0; j < n; ++j) {
            int64_t ij = i + j;
            total += (1.0 / (double)(ij * (ij + 1) / 2 + j + 1)) * u[j];
        }
        out[i] = total;
    }
}
