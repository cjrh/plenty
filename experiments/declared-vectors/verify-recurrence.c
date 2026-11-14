#define _GNU_SOURCE
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

void recurrence(double *, const double *, double, int64_t);

__attribute__((noinline))
static void reference(double *state, const double *delta, double scale, int64_t n) {
    for (int64_t i = 0; i < n; ++i) {
        state[0] = state[0] * scale + delta[0];
        state[1] = state[1] * scale + delta[1];
        state[2] = state[2] * scale + delta[2];
    }
}

static int same(double a, double b) {
    return (isnan(a) && isnan(b)) || memcmp(&a, &b, sizeof(a)) == 0;
}

int main(void) {
    const double values[] = { 0.0, -0.0, 1.0, -1.0, 0x1p-1074, -0x1p-1074,
        0x1p-1022, 0x1.fffffffffffffp1023, INFINITY, -INFINITY, NAN };
    unsigned count = 0;
    for (size_t a = 0; a < sizeof(values) / sizeof(*values); ++a) {
        for (size_t b = 0; b < sizeof(values) / sizeof(*values); ++b) {
            double expected[] = { values[a], values[b], -values[a] };
            double got[] = { values[a], values[b], -values[a] };
            double delta[] = { values[b], -values[a], values[a] };
            feclearexcept(FE_ALL_EXCEPT);
            reference(expected, delta, values[b], 3);
            int expected_invalid = fetestexcept(FE_INVALID);
            feclearexcept(FE_ALL_EXCEPT);
            recurrence(got, delta, values[b], 3);
            int actual_invalid = fetestexcept(FE_INVALID);
            for (int lane = 0; lane < 3; ++lane) {
                if (!same(expected[lane], got[lane]) || expected_invalid != actual_invalid) {
                    fprintf(stderr, "mismatch %zu %zu lane%d invalid %d/%d\n", a, b, lane,
                            expected_invalid, actual_invalid);
                    return 1;
                }
            }
            ++count;
        }
    }
    // Infinity multiplies only real lanes: private +0 padding must use scale1.
    double state[] = { 1.0, 2.0, 3.0 }, delta[] = { 0.0, 0.0, 0.0 };
    feclearexcept(FE_ALL_EXCEPT);
    recurrence(state, delta, INFINITY, 1);
    if (fetestexcept(FE_INVALID)) return 2;
    // Three unaligned doubles end exactly at an unreadable page boundary.
    long page = sysconf(_SC_PAGESIZE);
    void *left = mmap(0, 2 * page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    void *right = mmap(0, 2 * page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (left == MAP_FAILED || right == MAP_FAILED) return 3;
    if (mprotect((char *)left + page, page, PROT_NONE) ||
        mprotect((char *)right + page, page, PROT_NONE)) return 4;
    double *a = (double *)((char *)left + page - 3 * sizeof(double));
    double *b = (double *)((char *)right + page - 3 * sizeof(double));
    for (int i = 0; i < 3; ++i) { a[i] = i + 1; b[i] = i + 2; }
    recurrence(a, b, 2.0, 1);
    for (int i = 0; i < 3; ++i) if (a[i] != 2.0 * (i + 1) + i + 2) return 5;
    munmap(left, 2 * page);
    munmap(right, 2 * page);
    recurrence(0, 0, INFINITY, 0);
    recurrence(0, 0, INFINITY, -1);
    printf("verified %u IEEE cases, exception parity, unaligned guard pages, empty access\n", count);
    return 0;
}
