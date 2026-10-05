// Shared immutable values. Runtime helpers borrow their arguments unless stated
// otherwise; constructors return one owned reference. Static literals are immortal.
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>

typedef struct PlentyObject {
  uint64_t refs;
  void (*destroy)(void *);
} PlentyObject;

static _Noreturn void plenty_value_error(const char *message) {
  fprintf(stderr, "error: %s\n", message);
  exit(1);
}
static void *plenty_alloc(size_t bytes) {
  void *p = calloc(1, bytes ? bytes : 1);
  if (!p) plenty_value_error("out of memory");
  return p;
}
void plenty_retain(void *value) {
  PlentyObject *object = value;
  if (!object || object->refs == UINT64_MAX) return;
  if (object->refs == UINT64_MAX - 1) plenty_value_error("reference count overflow");
  ++object->refs;
}
static PlentyObject *pending;
static int dropping;
void plenty_release(void *value) {
  // Intrusive destruction queue avoids recursive native stack growth.
  PlentyObject *object = value;
  if (!object || object->refs == UINT64_MAX) return;
  if (--object->refs != 0) return;
  object->refs = (uint64_t)(uintptr_t)pending;
  pending = object;
  if (dropping) return;
  dropping = 1;
  while (pending) {
    object = pending;
    pending = (PlentyObject *)(uintptr_t)object->refs;
    object->destroy(object);
  }
  dropping = 0;
}

typedef struct PlentyStr {
  PlentyObject object;
  uint64_t byte_len, scalar_len;
  unsigned char data[];
} PlentyStr;
_Static_assert(sizeof(PlentyObject) == 16, "Plenty requires a 64-bit host");
_Static_assert(offsetof(PlentyStr, data) == 32, "string literal ABI");

// Validate external UTF-8 and count scalar values, including U+0000. Reject
// overlong encodings, surrogates, truncated sequences, and values above U+10FFFF.
static size_t plenty_utf8_count(const unsigned char *s, size_t len) {
  size_t count = 0;
  for (size_t i = 0; i < len; ++count) {
    unsigned c = s[i++];
    if (c < 0x80) continue;
    unsigned remaining, value, minimum;
    if (c >= 0xc2 && c <= 0xdf) { remaining = 1; value = c & 31; minimum = 0x80; }
    else if (c >= 0xe0 && c <= 0xef) { remaining = 2; value = c & 15; minimum = 0x800; }
    else if (c >= 0xf0 && c <= 0xf4) { remaining = 3; value = c & 7; minimum = 0x10000; }
    else plenty_value_error("invalid UTF-8 input");
    if (remaining > len - i) plenty_value_error("invalid UTF-8 input");
    while (remaining--) {
      c = s[i++];
      if ((c & 0xc0) != 0x80) plenty_value_error("invalid UTF-8 input");
      value = (value << 6) | (c & 63);
    }
    if (value < minimum || value > 0x10ffff || (value >= 0xd800 && value <= 0xdfff))
      plenty_value_error("invalid UTF-8 input");
  }
  return count;
}
static PlentyStr *plenty_str_new(const void *data, size_t len) {
  if (len > INT64_MAX || len > SIZE_MAX - sizeof(PlentyStr))
    plenty_value_error("string capacity overflow");
  PlentyStr *s = plenty_alloc(sizeof(*s) + len);
  s->object = (PlentyObject){1, free};
  s->byte_len = len;
  s->scalar_len = plenty_utf8_count(data, len);
  if (len) memcpy(s->data, data, len);
  return s;
}
const PlentyStr *plenty_concat(const PlentyStr *a, const PlentyStr *b) {
  if (b->byte_len > INT64_MAX - a->byte_len ||
      a->byte_len + b->byte_len > SIZE_MAX - sizeof(PlentyStr))
    plenty_value_error("string capacity overflow");
  size_t len = a->byte_len + b->byte_len;
  PlentyStr *s = plenty_alloc(sizeof(*s) + len);
  s->object = (PlentyObject){1, free};
  s->byte_len = len;
  s->scalar_len = a->scalar_len + b->scalar_len;
  memcpy(s->data, a->data, a->byte_len);
  memcpy(s->data + a->byte_len, b->data, b->byte_len);
  return s;
}
int8_t plenty_str_eq(const PlentyStr *a, const PlentyStr *b) {
  return a == b || (a->byte_len == b->byte_len && !memcmp(a->data, b->data, a->byte_len));
}
int8_t plenty_contains(const PlentyStr *haystack, const PlentyStr *needle) {
  if (needle->byte_len > haystack->byte_len) return 0;
  for (size_t i = 0; i <= haystack->byte_len - needle->byte_len; ++i)
    if (!memcmp(haystack->data + i, needle->data, needle->byte_len)) return 1;
  return 0;
}
void plenty_println(const PlentyStr *s) {
  fwrite(s->data, 1, s->byte_len, stdout);
  fputc('\n', stdout);
}
static void plenty_str_repr(const PlentyStr *s, int legacy) {
  fputc('"', stdout);
  for (size_t i = 0; i < s->byte_len; ++i) {
    unsigned char c = s->data[i];
    switch (c) {
    case '"': fputs("\\\"", stdout); break;
    case '\\': fputs("\\\\", stdout); break;
    case '\n': fputs("\\n", stdout); break;
    case '\r': fputs("\\r", stdout); break;
    case '\t': fputs("\\t", stdout); break;
    case 0: fputs("\\0", stdout); break;
    default:
      if (c < 0x20 || (legacy && c > 0x7e)) fprintf(stdout, "\\u{%x}", (unsigned)c);
      else fputc(c, stdout);
    }
  }
  fputc('"', stdout);
}
void plenty_print_str(const PlentyStr *s) { plenty_str_repr(s, 1); }

// getline is only an input adapter. Its trailing C terminator is discarded.
const PlentyStr *plenty_readline(void) {
  char *line = NULL;
  size_t capacity = 0;
  ssize_t n = getline(&line, &capacity, stdin);
  if (n < 0) { free(line); return NULL; }
  if (n && line[n - 1] == '\n') {
    --n;
    if (n && line[n - 1] == '\r') --n;
  }
  PlentyStr *s = plenty_str_new(line, (size_t)n);
  free(line);
  return s;
}
