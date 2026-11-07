/* Cache-hot bounded copy-out comparison, not a zero-copy mmap benchmark. */
#define _GNU_SOURCE
#include "memmap.h"
#include <assert.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <unistd.h>
static double now(void) {
    struct timespec t;
    assert(clock_gettime(CLOCK_MONOTONIC, &t) == 0);
    return (double)t.tv_sec + (double)t.tv_nsec / 1e9;
}
int main(void) {
    const size_t size = 64 * 1024 * 1024, block = 64 * 1024;
    char path[] = "/tmp/plenty-memmap-bench-XXXXXX";
    int fd = mkstemp(path);
    assert(fd >= 0 && ftruncate(fd, (off_t)size) == 0);
    unsigned char *buffer = malloc(block);
    assert(buffer != NULL);
    mm_mapping *mapping = NULL;
    assert(mm_open_v1(path, size, &mapping) == 0);
    /* Warm both paths; reuse exactly the same caller-owned output allocation. */
    for (size_t off = 0; off < size; off += block) {
        assert(pread(fd, buffer, block, (off_t)off) == (ssize_t)block);
        assert(mm_read_v1(mapping, off, buffer, block, 1) == (int64_t)block);
    }
    for (int round = 0; round < 4; ++round) {
        for (int pass = 0; pass < 2; ++pass) {
            int mapped = (round + pass) % 2;
            double start = now();
            for (int repeat = 0; repeat < 16; ++repeat) {
                for (size_t off = 0; off < size; off += block) {
                    int64_t n = mapped ? mm_read_v1(mapping, off, buffer, block, 1)
                                      : pread(fd, buffer, block, (off_t)off);
                    assert(n == (int64_t)block);
                }
            }
            printf("%s round=%d GiB/s=%.3f\n", mapped ? "mapped-copy" : "pread", round, 1.0 / (now() - start));
        }
    }
    mm_drop_v1(mapping);
    free(buffer);
    close(fd);
    unlink(path);
    return 0;
}
