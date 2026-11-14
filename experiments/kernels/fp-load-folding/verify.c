// Differential control for issue #980's raw CLIF kernels. Their public entry
// expects two native I128 view descriptors (pointer low, element count high).
// Cold diagnostic calls are replaced with ud2 only for this valid-span probe.
#define _GNU_SOURCE
#include <fenv.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

typedef unsigned __int128 View;
typedef void (*Kernel)(View, View);
extern void baseline_a(View, View), candidate_a(View, View);
extern void baseline_at(View, View), candidate_at(View, View);

static View view(const double *p, size_t n) {
    return (View)(uintptr_t)p | ((View)n << 64);
}

static double *guarded(size_t n, void **mapping, size_t *bytes) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    if (page < n * sizeof(double) || page % sizeof(double)) abort();
    *bytes = 2 * page;
    *mapping = mmap(NULL, *bytes, PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (*mapping == MAP_FAILED || mprotect((char *)*mapping + page, page, PROT_NONE)) abort();
    return (double *)((char *)*mapping + page) - n;
}

static void check_prefix(const void *mapping, size_t prefix, unsigned char expected) {
    const unsigned char *bytes = mapping;
    for (size_t j = 0; j < prefix; ++j) {
        if (bytes[j] != expected) {
            fprintf(stderr, "output prefix overwritten at %zu\n", j);
            exit(1);
        }
    }
}

static uint64_t random_bits(void) {
    static uint64_t state = UINT64_C(0x2676b5e59d9124a3);
    state ^= state << 13; state ^= state >> 7; state ^= state << 17;
    return state;
}

static void fail(unsigned case_id, int mode, unsigned transpose, size_t n,
                 int expected_flags, int actual_flags) {
    fprintf(stderr, "mismatch case=%u mode=%d transpose=%u n=%zu flags=%x/%x\n",
            case_id, mode, transpose, n, expected_flags, actual_flags);
    exit(1);
}

int main(void) {
    const uint64_t ieee[] = {
        0, UINT64_C(0x8000000000000000), 1, UINT64_C(0x8000000000000001),
        UINT64_C(0x0010000000000000), UINT64_C(0x3ff0000000000000),
        UINT64_C(0xbff0000000000000), UINT64_C(0x7fefffffffffffff),
        UINT64_C(0xffefffffffffffff), UINT64_C(0x7ff0000000000000),
        UINT64_C(0xfff0000000000000), UINT64_C(0x7ff8000000001234),
        UINT64_C(0xfff8000000005678), UINT64_C(0x7ff0000000000001),
        UINT64_C(0xfff0000000000002)
    };
    const int rounding[] = {FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO};
    unsigned verified = 0;
    for (unsigned m = 0; m < sizeof(rounding) / sizeof(rounding[0]); ++m) {
        if (fesetround(rounding[m])) abort();
        for (unsigned case_id = 0; case_id < 256; ++case_id) {
            size_t n = case_id % 65;
            void *input_mapping, *output_mapping;
            size_t input_bytes, output_bytes;
            double *input = guarded(n, &input_mapping, &input_bytes);
            double *output = guarded(n, &output_mapping, &output_bytes);
            unsigned char expected[65 * sizeof(double)];
            for (size_t j = 0; j < n; ++j) {
                uint64_t bits = case_id < 128 ? ieee[(case_id + j) % 15] : random_bits();
                memcpy(input + j, &bits, sizeof(bits));
            }
            // Protect source writes too; n==0 deliberately points into PROT_NONE.
            size_t page = (size_t)sysconf(_SC_PAGESIZE);
            if (mprotect(input_mapping, page, PROT_READ)) abort();
            for (unsigned transpose = 0; transpose < 2; ++transpose) {
                Kernel before = transpose ? baseline_at : baseline_a;
                Kernel after = transpose ? candidate_at : candidate_a;
                memset(output_mapping, 0xa5, page);
                feclearexcept(FE_ALL_EXCEPT);
                before(view(input, n), view(output, n));
                int expected_flags = fetestexcept(FE_ALL_EXCEPT);
                check_prefix(output_mapping, page - n * sizeof(double), 0xa5);
                memcpy(expected, output, n * sizeof(double));
                memset(output_mapping, 0x5a, page);
                feclearexcept(FE_ALL_EXCEPT);
                after(view(input, n), view(output, n));
                int actual_flags = fetestexcept(FE_ALL_EXCEPT);
                check_prefix(output_mapping, page - n * sizeof(double), 0x5a);
                if (expected_flags != actual_flags || memcmp(expected, output, n * sizeof(double)))
                    fail(case_id, rounding[m], transpose, n, expected_flags, actual_flags);
                ++verified;
            }
            if (munmap(input_mapping, input_bytes) || munmap(output_mapping, output_bytes)) abort();
        }
    }
    if (fesetround(FE_TONEAREST)) abort();
    // Over-limit guard must return before touching either invalid descriptor.
    baseline_a(view(NULL, 1000001), view(NULL, 0));
    candidate_a(view(NULL, 1000001), view(NULL, 0));
    baseline_at(view(NULL, 1000001), view(NULL, 0));
    candidate_at(view(NULL, 1000001), view(NULL, 0));
    printf("verified %u bit-exact matrix cases, four rounding modes, exception parity, guarded spans and over-limit early return\n", verified);
}
