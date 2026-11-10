#include <stdint.h>
#include <stddef.h>

#define IM 139968
#define IA 3877
#define IC 29573

struct rng { int64_t seed; };

void fill(struct rng *rng, int64_t *out, size_t n) {
    for (size_t i = 0; i < n; i++) {
        rng->seed = (rng->seed * IA + IC) % IM;
        out[i] = rng->seed;
    }
}

int64_t fill_local(int64_t seed, int64_t *out, size_t n) {
    int64_t s = seed % IM;
    for (size_t i = 0; i < n; i++) {
        s = (s * IA + IC) % IM;
        out[i] = s;
    }
    return s;
}
