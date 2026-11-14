#include <stdint.h>
// Bounded Seed guarantees 0..139967, so signed/unsigned recurrence agrees.
void fill(uint64_t *seed, uint64_t *out, int64_t n) {
    for (int64_t i = 0; i < n; ++i) {
        *seed = (*seed * 3877 + 29573) % 139968;
        out[i] = *seed;
    }
}
int64_t sum_accessor(const int64_t *value, int64_t n) {
    int64_t total = 0;
    for (int64_t i = 0; i < n; ++i) {
        if (__builtin_add_overflow(total, *value, &total)) __builtin_trap();
    }
    return (int64_t)total;
}
