/* Failure injection for `plenty test` and --test-hooks executables; see "Failure injection"
 * in book/src/design/stdlib/test.md. The compiler links this object only into
 * instrumented builds, with `-Wl,--wrap=NAME` for each call below, so the program and
 * runtime reach these definitions instead of the C library's. Default builds
 * never contain it.
 *
 * Each budget belongs to the calling thread. A negative budget never fails;
 * otherwise each call spends one unit and fails once none remain, until the
 * test restores the budget. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <unistd.h>

#define LOCAL __attribute__((tls_model("initial-exec")))

static _Thread_local LOCAL int64_t allocations = -1;
static _Thread_local LOCAL int64_t files = -1;
static _Thread_local LOCAL int file_error = 0;

static int spend(int64_t *budget) {
    if (*budget < 0) return 0;
    if (*budget == 0) return 1;
    *budget -= 1;
    return 0;
}

/* Standard streams and sockets are not files: a failing test must still be
 * able to report, and std.net has its own fakes. */
static int regular(int fd) {
    struct stat st;
    return fd > 2 && fstat(fd, &st) == 0 && S_ISREG(st.st_mode);
}

static int file_fails(int fd) {
    return files >= 0 && regular(fd) && spend(&files);
}

static int path_fails(void) {
    return files >= 0 && spend(&files);
}

/* kind 0 arms allocations and 1 arms file calls. A negative count restores
 * the call; `code` is the errno a failed file call reports. */
int8_t plenty_test_fault(int64_t kind, int64_t count, int32_t code) {
    if (kind == 0) {
        allocations = count < 0 ? -1 : count;
    } else {
        files = count < 0 ? -1 : count;
        file_error = code;
    }
    return 1;
}

void *__real_malloc(size_t);
void *__real_calloc(size_t, size_t);
void *__real_realloc(void *, size_t);
int __real_posix_memalign(void **, size_t, size_t);

void *__wrap_malloc(size_t size) {
    if (spend(&allocations)) { errno = ENOMEM; return NULL; }
    return __real_malloc(size);
}

void *__wrap_calloc(size_t count, size_t size) {
    if (spend(&allocations)) { errno = ENOMEM; return NULL; }
    return __real_calloc(count, size);
}

void *__wrap_realloc(void *old, size_t size) {
    if (spend(&allocations)) { errno = ENOMEM; return NULL; }
    return __real_realloc(old, size);
}

int __wrap_posix_memalign(void **out, size_t align, size_t size) {
    if (spend(&allocations)) return ENOMEM;
    return __real_posix_memalign(out, align, size);
}

#define FAIL_FILE() do { errno = file_error; return -1; } while (0)

int __real_open(const char *, int, ...);
int __real_open64(const char *, int, ...);
int __real_openat(int, const char *, int, ...);

static mode_t mode_of(int flags, va_list args) {
    return (flags & (O_CREAT | O_TMPFILE)) ? (mode_t)va_arg(args, int) : 0;
}

int __wrap_open(const char *path, int flags, ...) {
    va_list args;
    va_start(args, flags);
    mode_t mode = mode_of(flags, args);
    va_end(args);
    if (path_fails()) FAIL_FILE();
    return __real_open(path, flags, mode);
}

int __wrap_open64(const char *path, int flags, ...) {
    va_list args;
    va_start(args, flags);
    mode_t mode = mode_of(flags, args);
    va_end(args);
    if (path_fails()) FAIL_FILE();
    return __real_open64(path, flags, mode);
}

int __wrap_openat(int dir, const char *path, int flags, ...) {
    va_list args;
    va_start(args, flags);
    mode_t mode = mode_of(flags, args);
    va_end(args);
    if (path_fails()) FAIL_FILE();
    return __real_openat(dir, path, flags, mode);
}

ssize_t __real_read(int, void *, size_t);
ssize_t __real_write(int, const void *, size_t);
ssize_t __real_pread64(int, void *, size_t, off_t);
ssize_t __real_pwrite64(int, const void *, size_t, off_t);
ssize_t __real_writev(int, const struct iovec *, int);
int __real_fsync(int);
int __real_ftruncate64(int, off_t);

ssize_t __wrap_read(int fd, void *buf, size_t len) {
    if (file_fails(fd)) FAIL_FILE();
    return __real_read(fd, buf, len);
}

ssize_t __wrap_write(int fd, const void *buf, size_t len) {
    if (file_fails(fd)) FAIL_FILE();
    return __real_write(fd, buf, len);
}

ssize_t __wrap_pread64(int fd, void *buf, size_t len, off_t at) {
    if (file_fails(fd)) FAIL_FILE();
    return __real_pread64(fd, buf, len, at);
}

ssize_t __wrap_pwrite64(int fd, const void *buf, size_t len, off_t at) {
    if (file_fails(fd)) FAIL_FILE();
    return __real_pwrite64(fd, buf, len, at);
}

ssize_t __wrap_writev(int fd, const struct iovec *iov, int count) {
    if (file_fails(fd)) FAIL_FILE();
    return __real_writev(fd, iov, count);
}

int __wrap_fsync(int fd) {
    if (file_fails(fd)) FAIL_FILE();
    return __real_fsync(fd);
}

int __wrap_ftruncate64(int fd, off_t len) {
    if (file_fails(fd)) FAIL_FILE();
    return __real_ftruncate64(fd, len);
}
