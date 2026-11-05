#include <stdint.h>
#include <stddef.h>

struct result { int64_t offset, work; };

// Same preconditions and packed-lane algorithm as prefix-packed.plenty.
// Unsigned state and masks preserve bit 63 and define the left shift.
struct result scan(const uint8_t *data, int64_t length, const uint64_t *masks,
                   int64_t start, int64_t width, uint64_t starts, uint64_t accept, int64_t maximum) {
    uint64_t state = 0;
    int64_t at = start, work = 0;
    while (at < length) {
        if (work >= maximum) return (struct result){-2, work};
        work++;
        state = ((state << 1) | starts) & masks[data[at]];
        at++;
        if (state & accept) return (struct result){at - width, work};
    }
    return (struct result){-1, work};
}
