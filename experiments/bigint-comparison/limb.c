#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

// Same views, lengths and checked high+carry contract as limb.plenty.
// No restrict qualifier or unchecked output-length precondition.
__attribute__((noinline)) uint64_t limb(const uint64_t *a, size_t n,
                                       uint64_t *out, size_t out_len,
                                       uint64_t word) {
    uint64_t carry = 0;
    for (size_t i = 0; i < n; ++i) {
        __uint128_t product = (__uint128_t)a[i] * word;
        uint64_t low = (uint64_t)product;
        uint64_t sum = low + carry;
        if (i >= out_len) abort();
        out[i] = sum;
        uint64_t high = product >> 64;
        if (__builtin_add_overflow(high, (uint64_t)(sum < low), &carry)) abort();
    }
    return carry;
}

int main(void) {
    uint64_t a[] = {UINT64_MAX, 0, UINT64_MAX, 17};
    uint64_t out[4];
    uint64_t carry = limb(a, 4, out, 4, UINT64_MAX);
    printf("%" PRIu64 " %" PRIu64 " %" PRIu64 " %" PRIu64 " %" PRIu64 "\n",
           out[0], out[1], out[2], out[3], carry);
}
