/* MIT OR Apache-2.0. Uses Linux mmap and process_vm_readv, not memmap2 code.
 * All file-backed memory reads happen in the kernel's fault-contained copy.
 * Never dereference mapping->data in userspace, even after fstat succeeds.
 */
#define _GNU_SOURCE
#include "memmap.h"
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <unistd.h>
#ifndef __linux__
#error "This experimental backend requires Linux"
#endif
struct mm_mapping { void *data; size_t length; };
const uint8_t *mm_discovery_v1(size_t *length) {
    static const uint8_t contract[] = "memmap-experiment-linux-v1:open,length,read-strided,drop";
    *length = sizeof(contract) - 1;
    return contract;
}
int32_t mm_open_v1(const char *path, uint64_t max_bytes, mm_mapping **out) {
    *out = NULL;
    int fd = open(path, O_RDONLY | O_CLOEXEC | O_NONBLOCK);
    if (fd < 0) return errno;
    struct stat st;
    if (fstat(fd, &st) < 0) {
        int error = errno; close(fd); return error;
    }
    if (!S_ISREG(st.st_mode) || st.st_size < 0) { close(fd); return EINVAL; }
    if ((uint64_t)st.st_size > max_bytes || (uint64_t)st.st_size > PTRDIFF_MAX) {
        close(fd); return EFBIG;
    }
    mm_mapping *mapping = malloc(sizeof(*mapping));
    if (mapping == NULL) { close(fd); return ENOMEM; }
    mapping->length = (size_t)st.st_size;
    mapping->data = NULL;
    if (mapping->length != 0) {
        mapping->data = mmap(NULL, mapping->length, PROT_READ, MAP_SHARED, fd, 0);
        if (mapping->data == MAP_FAILED) {
            int error = errno; close(fd); free(mapping); return error;
        }
    }
    close(fd);
    *out = mapping;
    return 0;
}
uint64_t mm_length_v1(const mm_mapping *mapping) { return mapping->length; }
int64_t mm_read_v1(const mm_mapping *mapping, uint64_t offset,
                   uint8_t *destination, size_t count, size_t stride) {
    if (offset > mapping->length || stride == 0) return -EINVAL;
    size_t available = mapping->length - (size_t)offset;
    if (count > available) count = available;
    if (count > SSIZE_MAX || (count != 0 && (count - 1) > SIZE_MAX / stride)) return -EINVAL;
    if (count == 0) return 0;
    /* Addresses are formed only within the originally allocated mapping.
     * Truncation can invalidate pages; process_vm_readv reports EFAULT or a
     * partial read instead of delivering SIGBUS to the calling program.
     * It is intentionally not memcpy, and no signal handler is installed.
     */
    uint8_t *source = (uint8_t *)mapping->data + (size_t)offset;
    if (stride == 1) {
        struct iovec local = {destination, count};
        struct iovec remote = {source, count};
        ssize_t n = process_vm_readv(getpid(), &local, 1, &remote, 1, 0);
        return n < 0 ? -(int64_t)errno : n;
    }
    /* Support the declared strided adapter without depending on Plenty layout. */
    for (size_t i = 0; i < count; ++i) {
        struct iovec local = {destination + i * stride, 1};
        struct iovec remote = {source + i, 1};
        ssize_t n = process_vm_readv(getpid(), &local, 1, &remote, 1, 0);
        if (n != 1) return i != 0 ? (int64_t)i : n < 0 ? -(int64_t)errno : 0;
    }
    return (int64_t)count;
}
void mm_drop_v1(mm_mapping *mapping) {
    if (mapping == NULL) return;
    if (mapping->length != 0) munmap(mapping->data, mapping->length);
    free(mapping);
}
