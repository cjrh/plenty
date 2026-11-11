#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

/* `fill` with the range stated the only way C can: an unsigned field. */
struct urng { uint64_t seed; };

void fill_bounded(struct urng *rng, uint64_t *out, size_t n) {
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

/* `kernels KERNEL ROUNDS`: run one kernel ROUNDS times over 4096 elements.
   Build with -fno-inline so the timed loops are the ones counted above. */
int main(int argc, char **argv) {
    static int64_t out[4096];
    if (argc < 3)
        return 2;
    const char *kernel = argv[1];
    int64_t rounds = atoll(argv[2]), last = 42;
    if (!strcmp(kernel, "fill")) {
        struct rng rng = {42};
        for (int64_t r = 0; r < rounds; r++)
            fill(&rng, out, 4096);
        last = rng.seed;
    } else if (!strcmp(kernel, "fill_bounded")) {
        struct urng rng = {42};
        for (int64_t r = 0; r < rounds; r++)
            fill_bounded(&rng, (uint64_t *)out, 4096);
        last = (int64_t)rng.seed;
    } else if (!strcmp(kernel, "fill_local")) {
        for (int64_t r = 0; r < rounds; r++)
            last = fill_local(last, out, 4096);
    } else if (!strcmp(kernel, "fill_arms")) {
        for (int64_t r = 0; r < rounds; r++)
            last = fill_arms(last, out, 4096);
    } else if (!strcmp(kernel, "fill_inner")) {
        last = fill_inner(last, out, 4096, rounds);
    } else if (!strcmp(kernel, "fill_unbounded")) {
        for (int64_t r = 0; r < rounds; r++)
            last = fill_unbounded(last, out, 4096);
    }
    printf("%s %lld\n", kernel, (long long)last);
    return 0;
}
