#include <stdint.h>
#include <stddef.h>

struct result { int64_t offset, work; };

// Same preconditions and algorithm as prefix.plenty; no restrict promise.
// Masks use only their low eight bits, so the signed shift cannot overflow.
struct result scan(const uint8_t *data, int64_t length, const int64_t *masks,
                   int64_t start, int64_t width, int64_t maximum) {
    int64_t state = 0, at = start, work = 0;
    int64_t accept = (int64_t)1 << (width - 1);
    while (at < length) {
        if (work >= maximum) return (struct result){-2, work};
        work++;
        state = ((state << 1) | 1) & masks[data[at]];
        at++;
        if (state & accept) return (struct result){at - width, work};
    }
    return (struct result){-1, work};
}
