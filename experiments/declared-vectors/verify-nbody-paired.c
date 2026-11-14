/* Native diagnostic contract: exactly five seven-double bodies, in place.
 * Compare logical values/flags, not NaN payload identity. No public C ABI.
 */
#define _GNU_SOURCE
#include <fenv.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

typedef struct { double x, y, z, vx, vy, vz, mass; } Body;
extern void advance(Body *, int64_t);
extern void staged(Body *, int64_t);
static uint64_t seed = 913;
static double next_value(void) {
    seed = seed * UINT64_C(6364136223846793005) + 1;
    return (double)(seed >> 11) * 0x1p-53;
}
static uint64_t bits(double value) {
    uint64_t result;
    memcpy(&result, &value, sizeof(result));
    return result;
}
static int equal(double left, double right) {
    return bits(left) == bits(right) || (isnan(left) && isnan(right));
}
int main(void) {
    const uint64_t special[] = {
        0, UINT64_C(0x8000000000000000), 1, UINT64_C(0x8000000000000001),
        UINT64_C(0x0010000000000000), UINT64_C(0x3ff0000000000000),
        UINT64_C(0xbff0000000000000), UINT64_C(0x7fefffffffffffff),
        UINT64_C(0xffefffffffffffff), UINT64_C(0x7ff0000000000000),
        UINT64_C(0xfff0000000000000), UINT64_C(0x7ff8000000001234),
        UINT64_C(0xfff8000000005678), UINT64_C(0x7ff0000000000001)
    };
    const int modes[] = {FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO};
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    if (sizeof(Body) != 7 * sizeof(double) || page < 5 * sizeof(Body)) abort();
    unsigned checked = 0;
    for (unsigned mode = 0; mode < sizeof(modes) / sizeof(modes[0]); ++mode) {
        if (fesetround(modes[mode])) abort();
        for (unsigned sample = 0; sample < 384; ++sample) {
            Body input[5], expected[5];
            for (unsigned i = 0; i < 5; ++i) {
                if (sample < 256) {
                    input[i] = (Body){10 * i + next_value(), next_value(), next_value(),
                        0.1 * next_value(), 0.1 * next_value(), 0.1 * next_value(), next_value()};
                } else {
                    uint64_t fields[7];
                    for (unsigned f = 0; f < 7; ++f)
                        fields[f] = special[(sample + 3 * i + f) % (sizeof(special) / sizeof(special[0]))];
                    memcpy(&input[i], fields, sizeof(fields));
                }
            }
            void *mapping = mmap(NULL, 2 * page, PROT_READ | PROT_WRITE,
                                 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
            if (mapping == MAP_FAILED || mprotect((char *)mapping + page, page, PROT_NONE)) abort();
            memset(mapping, 0xa5, page);
            Body *actual = (Body *)((char *)mapping + page - sizeof(input));
            memcpy(actual, input, sizeof(input));
            memcpy(expected, input, sizeof(input));
            int64_t steps = (int64_t)(sample % 5) - 1;
            feclearexcept(FE_ALL_EXCEPT);
            staged(expected, steps);
            int expected_flags = fetestexcept(FE_ALL_EXCEPT);
            feclearexcept(FE_ALL_EXCEPT);
            advance(actual, steps);
            int actual_flags = fetestexcept(FE_ALL_EXCEPT);
            if (expected_flags != actual_flags) {
                fprintf(stderr, "flags mismatch mode=%u sample=%u steps=%ld flags=%x/%x\n",
                        mode, sample, (long)steps, expected_flags, actual_flags);
                return 1;
            }
            for (unsigned i = 0; i < 5; ++i) {
                double wanted[7], got[7];
                memcpy(wanted, &expected[i], sizeof(wanted));
                memcpy(got, &actual[i], sizeof(got));
                for (unsigned f = 0; f < 7; ++f) {
                    if (!equal(wanted[f], got[f])) {
                        fprintf(stderr, "value mismatch mode=%u sample=%u body=%u field=%u steps=%ld\n",
                                mode, sample, i, f, (long)steps);
                        return 1;
                    }
                }
                if (bits(input[i].mass) != bits(actual[i].mass)) {
                    fprintf(stderr, "immutable mass changed\n");
                    return 1;
                }
            }
            for (size_t i = 0; i < page - sizeof(input); ++i)
                if (((unsigned char *)mapping)[i] != 0xa5) {
                    fprintf(stderr, "prefix canary changed\n");
                    return 1;
                }
            if (munmap(mapping, 2 * page)) abort();
            ++checked;
        }
    }
    advance(NULL, 0);
    advance(NULL, -1);
    if (fesetround(FE_TONEAREST)) abort();
    printf("%u full-state cases: exact non-NaN bits, NaN classification, all FP flags, four rounding modes, guarded unaligned state and no-access nonpositive counts\n", checked);
    return 0;
}
