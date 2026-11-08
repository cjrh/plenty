// Test-only native ABI fixture, not a public C interface. A read-only owner
// borrows a 16-byte slot containing the header pointer. The packed header's
// len/capacity/data offsets are asserted in plenty-runtime/src/clist_layout.rs.
#define _GNU_SOURCE
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

// The fixture calls internal Tail code, not Plenty's public C ABI. ARM Tail
// reserves x0/x1 even for a scalar return. These test-only adapters translate
// the AAPCS64 arguments to x2..x5 and return x2 through the native x0 register.
#ifdef __aarch64__
extern int64_t plenty_test_simd_count(unsigned __int128, int32_t, int32_t);
extern int64_t plenty_test_scalar_count(unsigned __int128, int32_t, int32_t);
#define SIMD_COUNT plenty_test_simd_count
#define SCALAR_COUNT plenty_test_scalar_count
#define TAIL_ADAPTER(name, target) \
    __asm__(".text\n.p2align 2\n.globl " #name "\n.type " #name ",%function\n" \
            #name ":\n" \
            "stp x29, x30, [sp, #-16]!\n" \
            "mov x29, sp\n" \
            "mov w5, w3\n" \
            "mov w4, w2\n" \
            "mov x2, x0\n" \
            "mov x3, x1\n" \
            "bl " #target "\n" \
            "mov x0, x2\n" \
            "ldp x29, x30, [sp], #16\n" \
            "ret\n.size " #name ", .-" #name "\n")
TAIL_ADAPTER(plenty_test_simd_count, __plenty_fn_simd_count);
TAIL_ADAPTER(plenty_test_scalar_count, __plenty_fn_scalar_count);
#undef TAIL_ADAPTER
#else
extern int64_t __plenty_fn_simd_count(unsigned __int128, int32_t, int32_t);
extern int64_t __plenty_fn_scalar_count(unsigned __int128, int32_t, int32_t);
#define SIMD_COUNT __plenty_fn_simd_count
#define SCALAR_COUNT __plenty_fn_scalar_count
#endif

#ifdef SCAN_NARROW
typedef uint8_t value_t;
#else
typedef int32_t value_t;
#endif
#define WIDTH sizeof(value_t)

#ifdef SCAN_RANGE
static const int32_t low = -1073741824, high = 1073741824;
#else
static const int32_t low = 5, high = INT32_MAX;
#endif

static void header(unsigned char *owner, size_t length) {
    memset(owner, 0, 40);
    uint64_t len = length, capacity = (length + 7) & ~(size_t)7;
    memcpy(owner + 16, &len, 8);
    memcpy(owner + 24, &capacity, 8);
}

static int64_t fill(unsigned char *owner, size_t length) {
    int64_t expected = 0;
    for (size_t i = 0; i < length; ++i) {
        // Mix sign and high bits without implementation-defined narrowing.
        uint32_t bits = (uint32_t)i * UINT32_C(2147483647);
#ifdef SCAN_NARROW
        value_t value = (value_t)bits;
        expected += value > 127;
#else
        int32_t value;
        memcpy(&value, &bits, 4);
#endif
        memcpy(owner + 40 + i * WIDTH, &value, WIDTH);
#ifdef SCAN_NARROW
#elif defined(SCAN_RANGE)
        expected += value >= low && value < high;
#else
        expected += value > low;
#endif
    }
    return expected;
}

static int64_t call(unsigned char *owner, int scalar) {
    unsigned __int128 slot = (uintptr_t)owner;
    unsigned __int128 reference = (uintptr_t)&slot;
    return scalar ? SCALAR_COUNT(reference, low, high)
                  : SIMD_COUNT(reference, low, high);
}

static void guarded(void) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    unsigned char *mapping = mmap(NULL, page * 3, PROT_NONE,
                                 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(mapping != MAP_FAILED);
    assert(mprotect(mapping + page, page, PROT_READ | PROT_WRITE) == 0);
    for (size_t n = 0; n <= 129; ++n) {
        // Header needs eight-byte alignment, so up to seven bytes of poison
        // can precede the guard. A 16-byte overread past the last full vector
        // traps, unless tail and poison fill exactly one vector (an i32
        // three-row tail, a u8 tail of 9 to 15 rows); the poison matches the
        // predicate, so it would then corrupt the count.
        size_t padding = (8 - n * WIDTH % 8) % 8;
        unsigned char *owner = mapping + page * 2 - padding - n * WIDTH - 40;
        assert((uintptr_t)owner % 8 == 0);
        header(owner, n);
        int64_t expected = fill(owner, n);
        if (padding) memset(owner + 40 + n * WIDTH, WIDTH == 1 ? 0xff : 0x7f, padding);
        assert(call(owner, 0) == expected);
        assert(call(owner, 1) == expected);
    }
    assert(call(NULL, 0) == 0);
    assert(munmap(mapping, page * 3) == 0);
    puts("guarded lengths 0..129 and null empty owner passed");
}

static uint64_t now(void) {
    struct timespec ts;
    assert(clock_gettime(CLOCK_MONOTONIC, &ts) == 0);
    return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
}

int main(int argc, char **argv) {
    if (argc == 1) { guarded(); return 0; }
    assert(argc == 4);
    size_t n = strtoull(argv[1], NULL, 10);
    size_t repeats = strtoull(argv[2], NULL, 10);
    int scalar = atoi(argv[3]);
    unsigned char *owner = malloc(40 + n * WIDTH);
    assert(owner);
    header(owner, n);
    int64_t expected = fill(owner, n), total = 0;
    uint64_t start = now();
    for (size_t r = 0; r < repeats; ++r) total += call(owner, scalar);
    uint64_t elapsed = now() - start;
    assert(total == expected * (int64_t)repeats);
    printf("%lld %llu\n", (long long)total, (unsigned long long)elapsed);
    free(owner);
}
