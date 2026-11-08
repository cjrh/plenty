#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

/* Input is nonempty valid UTF-8, like the checked template token. */
static size_t width(unsigned char first) {
    return first < 0x80 ? 1 : first < 0xe0 ? 2 : first < 0xf0 ? 3 : 4;
}

int64_t scan(const unsigned char *text, size_t length) {
    size_t quote_width = width(text[0]);
    int64_t matches = 0;
    for (size_t at = 0; at < length;) {
        size_t scalar_width = width(text[at]);
        matches += scalar_width == quote_width &&
                   memcmp(text + at, text, scalar_width) == 0;
        at += scalar_width;
    }
    return matches;
}

int main(void) {
    unsigned char text[32768];
    memset(text, 'a', sizeof(text));
    printf("%lld\n", (long long)scan(text, sizeof(text)));
}
