#include <stdint.h>
double sum_index(const double *xs, int64_t n) { double t = 0; for (int64_t i = 0; i < n; i++) t += xs[i]; return t; }
double sum_iter(const double *xs, int64_t len) { double t = 0; for (const double *p = xs; p < xs + len; p++) t += *p; return t; }
float sum_f32(const float *xs, int64_t n) { float t = 0; for (int64_t i = 0; i < n; i++) t += xs[i]; return t; }
