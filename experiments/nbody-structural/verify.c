/* Differential check of the raw kernel's five-body pointer contract. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

typedef struct { double x, y, z, vx, vy, vz, mass; } Body;
extern void KERNEL(Body *, int64_t);
extern void staged(Body *, int64_t);
static uint64_t seed = 913;
static double next_value(void) {
    seed = seed * UINT64_C(6364136223846793005) + 1;
    return (double)(seed >> 11) * 0x1p-53;
}
int main(void) {
    for (int sample = 0; sample < 256; ++sample) {
        Body expected[5], actual[5];
        for (int i = 0; i < 5; ++i) {
            expected[i] = (Body){10*i+next_value(), next_value(), next_value(),
                0.1*next_value(), 0.1*next_value(), 0.1*next_value(), next_value()};
        }
        memcpy(actual, expected, sizeof(actual));
        int64_t steps = sample % 33 - 1;
        staged(expected, steps);
        KERNEL(actual, steps);
        if (memcmp(actual, expected, sizeof(actual))) {
            fprintf(stderr, "State mismatch at sample %d, steps %ld\n", sample, (long)steps);
            return 1;
        }
    }
    puts("256 randomized finite-state cases bit-identical");
    return 0;
}
