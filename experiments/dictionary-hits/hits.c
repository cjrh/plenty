// The floor for hits-K-N.plenty: SipHash-1-3 of the key bytes with a random
// key, a linear probe over (hash tag | entry) words, and a byte comparison of
// the stored key. Build: gcc -O2 hits.c; run: hits K PASSES
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define ROTL(x, b) (((x) << (b)) | ((x) >> (64 - (b))))
#define ROUND(v) do { \
    v[0] += v[1]; v[1] = ROTL(v[1], 13); v[1] ^= v[0]; v[0] = ROTL(v[0], 32); \
    v[2] += v[3]; v[3] = ROTL(v[3], 16); v[3] ^= v[2]; \
    v[0] += v[3]; v[3] = ROTL(v[3], 21); v[3] ^= v[0]; \
    v[2] += v[1]; v[1] = ROTL(v[1], 17); v[1] ^= v[2]; v[2] = ROTL(v[2], 32); } while (0)

static uint64_t k0 = 0x0706050403020100ull, k1 = 0x0f0e0d0c0b0a0908ull;

static uint64_t sip13(const unsigned char *data, size_t len) {
    uint64_t v[4] = {k0 ^ 0x736f6d6570736575ull, k1 ^ 0x646f72616e646f6dull,
                     k0 ^ 0x6c7967656e657261ull, k1 ^ 0x7465646279746573ull};
    size_t i = 0;
    for (; i + 8 <= len; i += 8) {
        uint64_t m;
        memcpy(&m, data + i, 8);
        v[3] ^= m; ROUND(v); v[0] ^= m;
    }
    uint64_t last = (uint64_t)len << 56;
    memcpy(&last, data + i, len - i);
    last |= (uint64_t)len << 56;
    v[3] ^= last; ROUND(v); v[0] ^= last;
    v[2] ^= 0xff; ROUND(v); ROUND(v); ROUND(v);
    return v[0] ^ v[1] ^ v[2] ^ v[3];
}

typedef struct { const unsigned char *key; size_t len; int64_t value; } Row;
static Row rows[8];
static uint64_t slots[8];
static size_t count;

__attribute__((noinline))
static int64_t *get_or_insert(const unsigned char *key, size_t len) {
    uint64_t hash = sip13(key, len), mask = 7, tag = hash << 3;
    for (size_t bucket = hash & mask;; bucket = (bucket + 1) & mask) {
        uint64_t slot = slots[bucket];
        if (slot == 0) {
            rows[count] = (Row){key, len, 0};
            slots[bucket] = tag | (count + 1);
            return &rows[count++].value;
        }
        Row *row = &rows[(slot & mask) - 1];
        if ((slot & ~mask) == tag && row->len == len && memcmp(row->key, key, len) == 0)
            return &row->value;
    }
}

int main(int argc, char **argv) {
    size_t k = strtoul(argv[1], NULL, 10), passes = strtoul(argv[2], NULL, 10);
    static const char *volatile text = "GGTATTTTAATTTATAGT";
    const unsigned char *key = (const unsigned char *)text;
    get_or_insert(key, k);
    get_or_insert((const unsigned char *)"ACG", 3);
    get_or_insert((const unsigned char *)"TTTATAGTGGTATTTTAA", 18);
    for (size_t i = 0; i < passes; i++)
        *get_or_insert(key, k) += 1;
    printf("%lld\n", (long long)*get_or_insert(key, k));
    return 0;
}
