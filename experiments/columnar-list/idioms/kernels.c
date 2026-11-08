/* Scalar reference kernels only: no restrict, fast-math or vectorization.
 * The AoS reference uses C's native struct, not Plenty's tagged list layout. */
#include <stddef.h>
#include <stdint.h>
struct Particle {
    double x, y, vx, vy, mass, temperature;
    int32_t health, team;
};
double list_mass(const struct Particle *p, size_t n) {
    double total = 0;
    for (size_t i = 0; i < n; ++i) total += p[i].mass;
    return total;
}
double clist_mass(const double *mass, size_t n) {
    double total = 0;
    for (size_t i = 0; i < n; ++i) total += mass[i];
    return total;
}
double list_hot_mass(const struct Particle *p, size_t n, double threshold) {
    double total = 0;
    for (size_t i = 0; i < n; ++i)
        if (p[i].temperature >= threshold) total += p[i].mass;
    return total;
}
double clist_hot_mass(const double *temperature, const double *mass, size_t n, double threshold) {
    double total = 0;
    for (size_t i = 0; i < n; ++i)
        if (temperature[i] >= threshold) total += mass[i];
    return total;
}
void list_advance(struct Particle *p, size_t n, double dt) {
    for (size_t i = 0; i < n; ++i) {
        p[i].x += dt * p[i].vx;
        p[i].y += dt * p[i].vy;
    }
}
void clist_advance(double *x, double *y, const double *vx, const double *vy, size_t n, double dt) {
    for (size_t i = 0; i < n; ++i) {
        x[i] += dt * vx[i];
        y[i] += dt * vy[i];
    }
}
