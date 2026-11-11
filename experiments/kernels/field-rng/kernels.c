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

int64_t fill_arms(int64_t seed, int64_t *out, size_t n) {
    int64_t s = seed % IM;
    for (size_t i = 0; i < n; i++) {
        if (i % 2 == 0)
            s = (s * IA + IC) % IM;
        else
            s = (s * 3 + 1) % 1000;
        out[i] = s;
    }
    return s;
}

int64_t fill_inner(int64_t seed, int64_t *out, size_t n, int64_t rounds) {
    int64_t s = seed % IM;
    for (int64_t r = 0; r < rounds; r++)
        for (size_t i = 0; i < n; i++) {
            s = (s * IA + IC) % IM;
            out[i] = s;
        }
    return s;
}

int64_t fill_unbounded(int64_t seed, int64_t *out, size_t n) {
    int64_t s = seed % IM;
    for (size_t i = 0; i < n; i++) {
        s = (s * IA + IC) % IM;
        if (s == 7)
            s = s + (int64_t)i;
        out[i] = s;
    }
    return s;
}
