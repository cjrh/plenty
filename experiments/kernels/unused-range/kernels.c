#include <stdint.h>
#include <stdbool.h>

double unused(int64_t n, double x) {
    for (int64_t i = 0; i < n; ++i)
        x = x * 1.0000001 + 0.5;
    return x;
}

bool escape(double cr, double ci) {
    double zr = 0.0, zi = 0.0, tr = 0.0, ti = 0.0;
    for (int64_t i = 0; i < 50; ++i) {
        if (!(tr + ti <= 4.0)) break;
        zi = 2.0 * zr * zi + ci;
        zr = tr - ti + cr;
        tr = zr * zr;
        ti = zi * zi;
    }
    return tr + ti <= 4.0;
}
