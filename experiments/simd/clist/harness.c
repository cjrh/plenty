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

extern int64_t __plenty_fn_simd_count(unsigned __int128, int32_t, int32_t);
extern int64_t __plenty_fn_scalar_count(unsigned __int128, int32_t, int32_t);

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
        int32_t value;
        memcpy(&value, &bits, 4);
        memcpy(owner + 40 + i * 4, &value, 4);
#ifdef SCAN_RANGE
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
    return scalar ? __plenty_fn_scalar_count(reference, low, high)
                  : __plenty_fn_simd_count(reference, low, high);
}

static void guarded(void) {
    size_t page = (size_t)sysconf(_SC_PAGESIZE);
    unsigned char *mapping = mmap(NULL, page * 3, PROT_NONE,
                                 MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(mapping != MAP_FAILED);
    assert(mprotect(mapping + page, page, PROT_READ | PROT_WRITE) == 0);
    for (size_t n = 0; n <= 129; ++n) {
        // Header needs eight-byte alignment. Odd lengths leave one scalar
        // of poison before the guard; every full vector overread still traps
        // except a three-row tail, whose poison would corrupt the count.
        size_t padding = (n & 1) * 4;
        unsigned char *owner = mapping + page * 2 - padding - n * 4 - 40;
        assert((uintptr_t)owner % 8 == 0);
        header(owner, n);
        int64_t expected = fill(owner, n);
        if (padding) memset(owner + 40 + n * 4, 0x7f, padding);
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
    unsigned char *owner = malloc(40 + n * 4);
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
