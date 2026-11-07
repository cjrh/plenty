#include <stdint.h>
#include <stdio.h>

/* The same exclusive byte buffer and signed length as &mut Bytes. */
void transform(uint8_t *data, int64_t len) {
    for (int64_t i = 0; i < len; ++i)
        data[i] ^= 32;
}

int main(void) {
    uint8_t data[] = "ACGT";
    transform(data, 4);
    printf("b\"%s\"\n", (char *)data);
}
