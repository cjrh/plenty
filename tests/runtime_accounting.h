// Injected only by the ownership integration test's temporary C compiler wrapper.
// Counts runtime allocations, not libc internals or immortal source literals.
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef struct TestAllocation { void *ptr; size_t size; struct TestAllocation *next; } TestAllocation;
static TestAllocation *test_allocations;
static size_t test_live_bytes;
static inline void *test_calloc(size_t n, size_t size) {
  void *p = calloc(n, size);
  if (p) {
    TestAllocation *record = malloc(sizeof(*record));
    if (!record) abort();
    *record = (TestAllocation){p, n * size, test_allocations};
    test_allocations = record;
    test_live_bytes += record->size;
  }
  return p;
}
static inline void test_free(void *ptr) {
  TestAllocation **at = &test_allocations;
  while (*at && (*at)->ptr != ptr) at = &(*at)->next;
  if (*at) {
    TestAllocation *record = *at;
    *at = record->next;
    test_live_bytes -= record->size;
    free(record);
  }
  free(ptr);
}
struct PlentyStr;
void plenty_test_println(const struct PlentyStr *s);
void plenty_println(const struct PlentyStr *s) {
  uint64_t len;
  memcpy(&len, (const unsigned char *)s + 16, 8);
  const char *checkpoint = "__test_small_live_heap__";
  if (len == strlen(checkpoint) && !memcmp((const unsigned char *)s + 32, checkpoint, len)) {
    if (test_live_bytes > 4096) {
      fprintf(stderr, "out-of-scope values retained: %zu bytes\n", test_live_bytes);
      abort();
    }
  }
  plenty_test_println(s);
}
int plenty_test_main(int argc, char **argv);
int main(int argc, char **argv) {
  int result = plenty_test_main(argc, argv);
  if (!result && test_allocations) {
    fprintf(stderr, "runtime leaked %zu bytes\n", test_live_bytes);
    return 99;
  }
  return result;
}
#define calloc test_calloc
#define free test_free
#define plenty_println plenty_test_println
#define main plenty_test_main
