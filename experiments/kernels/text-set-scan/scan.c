#include <stdint.h>

typedef unsigned __int128 Option;
typedef struct { const unsigned char *data; uint64_t len; } View;

/* Same read-only byte ranges and scalar-relative result as the Plenty kernel.
 * The runtime algorithm is unchanged; isolate only its caller/ABI overhead. */
extern int64_t plenty_text_view_find_in(const unsigned char *, uint64_t,
                                      const unsigned char *, uint64_t, uint64_t);

Option scan(View text, View set) {
    int64_t found = plenty_text_view_find_in(text.data, text.len, set.data, set.len, 1);
    return found < 0 ? 0 : ((Option)1 << 64) | (uint64_t)found;
}

#ifdef SCAN_BENCHMARK
#include <stdio.h>
int main(void) {
    const unsigned char text[] = {0xc3, 0xa9, ';', 0xf0, 0x9f, 0x99, 0x82};
    const unsigned char set[] = {';'};
    uint64_t sum = 0;
    for (unsigned i = 0; i < 1000000; ++i)
        sum += (uint64_t)scan((View){text, sizeof text}, (View){set, sizeof set});
    printf("%llu\n", (unsigned long long)sum);
}
#endif
