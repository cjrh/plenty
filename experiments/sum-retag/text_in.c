/* std.bytes.text_in, written in C against the same runtime call. See
   README.md. */
#include <stdint.h>
typedef struct { uint64_t word, tags; } sum;
extern void plenty_op_183(void *args, const void *descriptor, const void *site);
extern const char result_metadata;

/* std.bytes.text_in with the same runtime call: one branch per outcome. */
sum text_in(const uint8_t *bytes, uint64_t len) {
    _Alignas(16) uint64_t args[12];
    args[0] = (uint64_t)bytes;
    args[1] = len;
    args[2] = args[3] = args[4] = args[5] = 0;
    plenty_op_183(args, &result_metadata, 0);
    uint64_t word = args[6], tags = args[7];
    if (tags & 1)            /* Err(AllocError) -> Err(BytesError.Allocation(e)) */
        return (sum){word, ((tags >> 1) << 2) | 3};
    if (tags & 2)            /* Ok(Some(text)) -> Ok(text) */
        return (sum){word, 0};
    return (sum){0, 1};      /* Ok(Nothing) -> Err(InvalidUtf8) */
}
