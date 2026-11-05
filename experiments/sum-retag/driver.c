/* Runs text_in.c's text_in on its Ok(Some) path. A separate translation
   unit, so gcc cannot inline text_in or the stub. See README.md. */
#include <stdint.h>
#include <stdio.h>
typedef struct { uint64_t word, tags; } sum;
sum text_in(const uint8_t *bytes, uint64_t len);
const char result_metadata = 0;

/* Stands in for the runtime conversion: Ok(Some(text)), tags 0b10. */
void plenty_op_183(void *args, const void *descriptor, const void *site) {
    uint64_t *words = args;
    (void)descriptor;
    (void)site;
    words[6] = words[0] ^ words[1];
    words[7] = 2;
}

int main(void) {
    static uint8_t bytes[4096];
    uint64_t total = 0;
    for (uint64_t i = 0; i + 4 <= sizeof bytes; i++)
        total ^= text_in(bytes + i, 4).tags;
    printf("%llu\n", (unsigned long long)total);
    return 0;
}
