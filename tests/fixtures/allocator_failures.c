/* Deterministic failures of the probe's real malloc calls, not its counters.
 * Armed only around one Plenty operation; never allocates to record a failure.
 */
#include <stdint.h>
#include <stddef.h>

static int64_t remaining = -1;
void allocator_fail_after(int64_t successes) { remaining = successes; }
void *__real_malloc(size_t size);
void *__wrap_malloc(size_t size) {
    if (remaining == 0) return NULL;
    if (remaining > 0) --remaining;
    return __real_malloc(size);
}
