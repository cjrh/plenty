#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

/* Keep matching function boundaries without whole-program constant folding. */
#define KERNEL __attribute__((noinline, noclone))

KERNEL uint64_t div10(uint64_t x) { return x / UINT64_C(10); }
KERNEL uint64_t rem10(uint64_t x) { return x % UINT64_C(10); }

KERNEL uint64_t mix(uint64_t seed, uint64_t count) {
    uint64_t x = seed;
    for (uint64_t i = 0; i < count; ++i) {
        x = (x ^ (x >> 13)) * UINT64_C(6364136223846793005);
        x = ((x << 17) | (x >> 47)) + UINT64_C(1442695040888963407);
    }
    return x;
}

static int64_t checked_add(int64_t a, int64_t b) {
    int64_t result;
    if (__builtin_add_overflow(a, b, &result)) abort();
    return result;
}

KERNEL int64_t escape(double cr, double ci, int64_t limit) {
    double zr = 0.0, zi = 0.0;
    int64_t iteration = 0;
    while (iteration < limit) {
        double rr = zr * zr, ii = zi * zi;
        if (rr + ii > 4.0) break;
        zi = (2.0 * zr) * zi + ci;
        zr = rr - ii + cr;
        iteration = checked_add(iteration, 1);
    }
    return iteration;
}

/* Pointer/count models an already valid borrowed contiguous collection. */
KERNEL uint64_t sum_words(const uint64_t *values, int64_t count) {
    uint64_t total = 0;
    for (int64_t i = 0; i < count; i = checked_add(i, 1)) total += values[i];
    return total;
}

KERNEL uint64_t sum_indexed(const uint64_t *values, int64_t count) {
    uint64_t total = 0;
    for (int64_t i = 0; i < count; i = checked_add(i, 1)) total += values[i];
    return total;
}

int main(void) {
    uint64_t inputs[] = {0, 1, 9, 10, 11, 123456789, UINT64_MAX};
    for (unsigned i = 0; i < sizeof(inputs) / sizeof(inputs[0]); ++i)
        printf("%" PRIu64 " %" PRIu64 "\n", div10(inputs[i]), rem10(inputs[i]));
    printf("%" PRIu64 " %" PRIu64 " %" PRIu64 "\n", mix(42, 0), mix(42, 1), mix(42, 10000));
    int64_t checksum = 0;
    for (int64_t y = 0; y < 48; ++y)
        for (int64_t x = 0; x < 64; ++x)
            checksum = checked_add(checksum, escape((double)x / 24.0 - 2.0, (double)y / 24.0 - 1.0, 128));
    printf("%" PRId64 " %" PRId64 " %" PRId64 "\n", checksum, escape(0, 0, 0), escape(0, 0, -1));
    uint64_t values[4096], overflow[] = {UINT64_MAX, 1, 9};
    for (uint64_t i = 0; i < 4096; ++i) values[i] = i * UINT64_C(123456789);
    printf("%" PRIu64 " %" PRIu64 " %" PRIu64 "\n", sum_words(NULL, 0), sum_words(values, 4096), sum_words(overflow, 3));
    printf("%" PRIu64 " %" PRIu64 " %" PRIu64 "\n", sum_indexed(NULL, 0), sum_indexed(values, 4096), sum_indexed(overflow, 3));
}
