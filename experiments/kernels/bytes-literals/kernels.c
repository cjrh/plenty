/* C counterparts of kernels.plenty, without bounds checks. */
#include <stdint.h>

static const uint8_t ALU[] =
    "GGCCGGGCGCGGTGGCTCACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGATCACCTGAGGTC"
    "AGGAGTTCGAGACCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAATACAAAAATTAGCCGGGCG"
    "TGGTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAGGCTGAGGCAGGAGAATCGCTTGAACCCGGGAGGCGG"
    "AGGTTGCAGTGAGCCGAGATCGCGCCACTGCACTCCAGCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAA";
#define ALU_LEN ((int64_t)sizeof ALU - 1)

int64_t fill(uint8_t *block, int64_t count, int64_t position) {
    for (int64_t i = 0; i < count; i++) {
        block[i] = ALU[position];
        position++;
        if (position == ALU_LEN)
            position = 0;
    }
    return position;
}

uint64_t checksum(void) {
    uint64_t total = 0;
    for (int64_t i = 0; i < ALU_LEN; i++)
        total += ALU[i];
    return total;
}
