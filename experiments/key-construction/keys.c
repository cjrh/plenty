// The floor for keys-K-N.plenty: a range check, an ASCII test and either a
// packed word (at most 7 bytes) or a heap copy. Build: gcc -O2 keys.c
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { uint64_t word; char *heap; } Key;

__attribute__((noinline))
static int key(const unsigned char *sequence, size_t len, size_t start, size_t stop, Key *out) {
    if (start > stop || stop > len) abort();
    size_t count = stop - start;
    const unsigned char *bytes = sequence + start;
    for (size_t i = 0; i < count; i++)
        if (bytes[i] & 0x80) return 0;
    if (count <= 7) {
        uint64_t word = 0;
        memcpy(&word, bytes, count);
        out->word = word << 8 | count << 1 | 1;
        out->heap = NULL;
    } else {
        out->heap = malloc(count + 1);
        if (!out->heap) abort();
        memcpy(out->heap, bytes, count);
        out->word = count << 1;
    }
    return 1;
}

int main(int argc, char **argv) {
    size_t k = strtoul(argv[1], NULL, 10), passes = strtoul(argv[2], NULL, 10);
    static const char *volatile text = "GGTATTTTAATTTATAGTACGTACGTAACCGGTT";
    const unsigned char *sequence = (const unsigned char *)text;
    size_t len = strlen(text), span = len - k + 1, total = 0, start = 0;
    for (size_t i = 0; i < passes; i++) {
        Key made;
        if (!key(sequence, len, start, start + k, &made)) abort();
        total += (made.word & 0xff) >> 1;
        free(made.heap);
        if (++start == span) start = 0;
    }
    printf("%zu\n", total);
    return 0;
}
