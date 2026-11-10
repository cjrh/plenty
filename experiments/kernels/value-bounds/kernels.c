// The loops of kernels.plenty in C. Build with
//   gcc -O2 -fno-tree-vectorize -c kernels.c
// so the comparison is with scalar code.
#include <stdbool.h>
#include <stdint.h>

void a_times_u(const double *u, double *v, int64_t n) {
    for (int64_t i = 0; i < n; i++) {
        double total = 0.0;
        for (int64_t j = 0; j < n; j++)
            total += u[j] / (double)((i + j) * (i + j + 1) / 2 + i + 1);
        v[i] = total;
    }
}

bool escape(double cr, double ci) {
    double zr = 0.0, zi = 0.0, tr = 0.0, ti = 0.0;
    for (int64_t i = 0; i < 50 && tr + ti <= 4.0; i++) {
        zi = 2.0 * zr * zi + ci;
        zr = tr - ti + cr;
        tr = zr * zr;
        ti = zi * zi;
    }
    return tr + ti <= 4.0;
}

int64_t select_codes(int64_t seed, const int64_t *limits, const uint8_t *codes,
                     uint8_t *out, int64_t n) {
    int64_t state = seed;
    for (int64_t i = 0; i < n; i++) {
        state = (state * 3877 + 29573) % 139968;
        int64_t k = 0;
        while (limits[k] < state)
            k++;
        out[i] = codes[k];
    }
    return state;
}
