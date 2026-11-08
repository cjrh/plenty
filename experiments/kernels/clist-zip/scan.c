#include <stddef.h>
double scan(const double *key, const double *weight, size_t len, double limit) {
    double total = 0.0;
    for (size_t i = 0; i < len; ++i)
        if (key[i] >= limit) total += weight[i];
    return total;
}
