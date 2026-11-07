#define _GNU_SOURCE
#include "memmap.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

int main(void) {
    char path[] = "/tmp/plenty-memmap-native-XXXXXX";
    int fd = mkstemp(path);
    assert(fd >= 0);
    const size_t page = (size_t)sysconf(_SC_PAGESIZE);
    const size_t size = page * 3;
    unsigned char *input = malloc(size);
    unsigned char *output = malloc(size);
    assert(input != NULL && output != NULL);
    memset(input, 0x6b, size);
    assert(write(fd, input, size) == (ssize_t)size);
    mm_mapping *mapping = NULL;
    assert(mm_open_v1(path, size - 1, &mapping) == EFBIG && mapping == NULL);
    assert(mm_open_v1(path, size, &mapping) == 0);
    assert(mm_length_v1(mapping) == size);
    assert(mm_read_v1(mapping, 0, output, size, 1) == (int64_t)size);
    assert(memcmp(input, output, size) == 0);
    unsigned char strided[8] = {0};
    assert(mm_read_v1(mapping, 0, strided, 4, 2) == 4);
    for (size_t i = 0; i < 8; ++i) assert(strided[i] == (i % 2 == 0 ? 0x6b : 0));
    assert(mm_read_v1(mapping, size, NULL, 10, 1) == 0);
    assert(mm_read_v1(mapping, size + 1, output, 1, 1) == -EINVAL);
    /* Removing every page must fail safely, including a previously faulted page. */
    assert(ftruncate(fd, 0) == 0);
    assert(mm_read_v1(mapping, 0, output, size, 1) == -EFAULT);
    assert(ftruncate(fd, (off_t)page) == 0);
    int64_t partial = mm_read_v1(mapping, 0, output, size, 1);
    assert(partial == (int64_t)page);
    assert(mm_read_v1(mapping, page, output, 1, 1) == -EFAULT);
    /* True concurrent truncation from another process: never a userspace SIGBUS. */
    pid_t child = fork();
    assert(child >= 0);
    if (child == 0) {
        for (int i = 0; i < 2000; ++i) {
            if (ftruncate(fd, 0) != 0 || ftruncate(fd, (off_t)size) != 0) _exit(1);
        }
        _exit(0);
    }
    for (int i = 0; i < 2000; ++i) {
        int64_t n = mm_read_v1(mapping, 0, output, size, 1);
        assert(n == -EFAULT || (n >= 0 && n <= (int64_t)size));
    }
    int status;
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    mm_drop_v1(mapping);
    assert(ftruncate(fd, 0) == 0);
    assert(mm_open_v1(path, 0, &mapping) == 0);
    assert(mm_length_v1(mapping) == 0);
    assert(mm_read_v1(mapping, 0, NULL, 0, 1) == 0);
    mm_drop_v1(mapping);
    mm_drop_v1(NULL);
    assert(mm_open_v1("/dev/null", 0, &mapping) == EINVAL && mapping == NULL);
    assert(mm_open_v1("/no/such/memmap-test-file", 0, &mapping) == ENOENT && mapping == NULL);
    close(fd);
    unlink(path);
    free(input);
    free(output);
    return 0;
}
