// Native collection storage. All source-visible collections are immutable
// values. Only compiler-private builders mutate in place. An update returns a
// new value. Allocations are retained until process exit, pending source-level
// ownership.
#include <inttypes.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct PlentyAllocation {
  void *data;
  struct PlentyAllocation *next;
} PlentyAllocation;
static PlentyAllocation *plenty_allocations;
static void plenty_collections_cleanup(void) {
  while (plenty_allocations) {
    PlentyAllocation *next = plenty_allocations->next;
    free(plenty_allocations->data);
    free(plenty_allocations);
    plenty_allocations = next;
  }
}
static _Noreturn void collection_error(const char *message) {
  fprintf(stderr, "error: %s\n", message);
  exit(1);
}
static void *collection_alloc(size_t count, size_t size) {
  static int registered;
  if (!registered) {
    atexit(plenty_collections_cleanup);
    registered = 1;
  }
  if (size && count > SIZE_MAX / size)
    collection_error("collection capacity overflow");
  void *data = calloc(count ? count : 1, size);
  PlentyAllocation *record = malloc(sizeof(*record));
  if (!data || !record)
    collection_error("out of memory");
  *record = (PlentyAllocation){data, plenty_allocations};
  plenty_allocations = record;
  return data;
}

typedef struct PlentyType {
  char kind;
  struct PlentyType *key, *value;
} PlentyType;
typedef struct PlentyEntry {
  uint64_t key, value;
} PlentyEntry;
typedef struct PlentyCollection {
  PlentyType *type;
  size_t len, capacity, table_capacity;
  PlentyEntry *entries;
  size_t *table; // entry index + 1, zero denotes an empty bucket
  int64_t start, stop, step;
} PlentyCollection;

static PlentyType *collection_type(const char **text) {
  PlentyType *t = collection_alloc(1, sizeof(*t));
  t->kind = *(*text)++;
  if (t->kind == 'L' || t->kind == 'S' || t->kind == 'D')
    t->key = collection_type(text);
  if (t->kind == 'D')
    t->value = collection_type(text);
  return t;
}
static PlentyCollection *collection_new(PlentyType *type) {
  PlentyCollection *c = collection_alloc(1, sizeof(*c));
  c->type = type;
  return c;
}
static uint64_t collection_hash(uint64_t value, const PlentyType *t) {
  if (t->kind == 's') {
    uint64_t h = UINT64_C(14695981039346656037);
    for (const unsigned char *p = (const unsigned char *)(uintptr_t)value; *p;
         ++p) {
      h ^= *p;
      h *= UINT64_C(1099511628211);
    }
    return h;
  }
  value ^= value >> 30;
  value *= UINT64_C(0xbf58476d1ce4e5b9);
  value ^= value >> 27;
  value *= UINT64_C(0x94d049bb133111eb);
  return value ^ (value >> 31);
}
static int collection_equal(const PlentyCollection *, const PlentyCollection *);
static int collection_value_equal(uint64_t a, uint64_t b, const PlentyType *t) {
  if (t->kind == 's')
    return strcmp((const char *)(uintptr_t)a, (const char *)(uintptr_t)b) == 0;
  if (t->kind == 'L' || t->kind == 'S' || t->kind == 'D' || t->kind == 'R')
    return collection_equal((const PlentyCollection *)(uintptr_t)a,
                            (const PlentyCollection *)(uintptr_t)b);
  return a == b;
}
static size_t collection_bucket(const PlentyCollection *c, uint64_t key) {
  size_t bucket = collection_hash(key, c->type->key) & (c->table_capacity - 1);
  while (c->table[bucket] &&
         !collection_value_equal(c->entries[c->table[bucket] - 1].key, key,
                                 c->type->key))
    bucket = (bucket + 1) & (c->table_capacity - 1);
  return bucket;
}
static size_t collection_find(const PlentyCollection *c, uint64_t key) {
  if (!c->table_capacity)
    return SIZE_MAX;
  size_t entry = c->table[collection_bucket(c, key)];
  return entry ? entry - 1 : SIZE_MAX;
}
static void collection_rehash(PlentyCollection *c, size_t capacity) {
  c->table = collection_alloc(capacity, sizeof(*c->table));
  c->table_capacity = capacity;
  for (size_t i = 0; i < c->len; ++i)
    c->table[collection_bucket(c, c->entries[i].key)] = i + 1;
}
static void collection_reserve(PlentyCollection *c) {
  if (c->len >= INT64_MAX)
    collection_error("collection capacity overflow");
  if (c->len == c->capacity) {
    size_t capacity = c->capacity ? c->capacity * 2 : 8;
    if (capacity < c->capacity)
      collection_error("collection capacity overflow");
    PlentyEntry *entries = collection_alloc(capacity, sizeof(*entries));
    if (c->len)
      memcpy(entries, c->entries, c->len * sizeof(*entries));
    c->entries = entries;
    c->capacity = capacity;
  }
  if (c->type->kind != 'L' && c->len >= c->table_capacity / 2) {
    size_t capacity = c->table_capacity ? c->table_capacity * 2 : 16;
    if (capacity < c->table_capacity)
      collection_error("collection capacity overflow");
    collection_rehash(c, capacity);
  }
}
static void collection_insert(PlentyCollection *c, uint64_t key,
                              uint64_t value) {
  if (c->type->kind != 'L') {
    size_t existing = collection_find(c, key);
    if (existing != SIZE_MAX) {
      c->entries[existing].value = value;
      return;
    }
  }
  collection_reserve(c);
  c->entries[c->len] = (PlentyEntry){key, value};
  if (c->type->kind != 'L')
    c->table[collection_bucket(c, key)] = c->len + 1;
  ++c->len;
}
static PlentyCollection *collection_copy(const PlentyCollection *source) {
  PlentyCollection *c = collection_new(source->type);
  for (size_t i = 0; i < source->len; ++i)
    collection_insert(c, source->entries[i].key, source->entries[i].value);
  return c;
}
static size_t collection_index(int64_t index, size_t len) {
  if (index < 0)
    index += (int64_t)len;
  if (index < 0 || (uint64_t)index >= len)
    collection_error("index out of bounds");
  return (size_t)index;
}
static uint64_t collection_at(const PlentyCollection *c, size_t i) {
  if (c->type->kind == 'R')
    return (uint64_t)((__int128)c->start + (__int128)i * c->step);
  return c->entries[i].key;
}
static int collection_equal(const PlentyCollection *a,
                            const PlentyCollection *b) {
  if (a == b)
    return 1;
  if (a->len != b->len)
    return 0;
  if (a->type->kind == 'R')
    return !a->len ||
           (a->start == b->start && (a->len == 1 || a->step == b->step));
  for (size_t i = 0; i < a->len; ++i) {
    if (a->type->kind == 'L') {
      if (!collection_value_equal(a->entries[i].key, b->entries[i].key,
                                  a->type->key))
        return 0;
    } else {
      size_t j = collection_find(b, a->entries[i].key);
      if (j == SIZE_MAX)
        return 0;
      if (a->type->kind == 'D' &&
          !collection_value_equal(a->entries[i].value, b->entries[j].value,
                                  a->type->value))
        return 0;
    }
  }
  return 1;
}
static void collection_print(const PlentyCollection *);
static void collection_print_value(uint64_t value, const PlentyType *t) {
  if (t->kind >= '1' && t->kind <= '4')
    printf("%" PRId64, (int64_t)value);
  else if (t->kind >= '5' && t->kind <= '8')
    printf("%" PRIu64, value);
  else if (t->kind == 'b')
    fputs(value ? "True" : "False", stdout);
  else if (t->kind == 's') {
    fputc('"', stdout);
    for (const unsigned char *p = (const unsigned char *)(uintptr_t)value; *p;
         ++p) {
      switch (*p) {
      case '"':
        fputs("\\\"", stdout);
        break;
      case '\\':
        fputs("\\\\", stdout);
        break;
      case '\n':
        fputs("\\n", stdout);
        break;
      case '\r':
        fputs("\\r", stdout);
        break;
      case '\t':
        fputs("\\t", stdout);
        break;
      default:
        fputc(*p, stdout);
      }
    }
    fputc('"', stdout);
  } else
    collection_print((const PlentyCollection *)(uintptr_t)value);
}
static void collection_print(const PlentyCollection *c) {
  if (c->type->kind == 'R') {
    printf("range(%" PRId64 ", %" PRId64 ", %" PRId64 ")", c->start, c->stop,
           c->step);
    return;
  }
  if (c->type->kind == 'S' && !c->len) {
    fputs("set()", stdout);
    return;
  }
  fputc(c->type->kind == 'L' ? '[' : '{', stdout);
  for (size_t i = 0; i < c->len; ++i) {
    if (i)
      fputs(", ", stdout);
    collection_print_value(c->entries[i].key, c->type->key);
    if (c->type->kind == 'D') {
      fputs(": ", stdout);
      collection_print_value(c->entries[i].value, c->type->value);
    }
  }
  fputc(c->type->kind == 'L' ? ']' : '}', stdout);
}
static size_t collection_text_length(const char *s) {
  size_t length = 0;
  for (const unsigned char *p = (const unsigned char *)s; *p; ++p)
    if ((*p & 0xc0) != 0x80)
      ++length;
  return length;
}
static uint64_t collection_text_at(const char *s, int64_t index) {
  size_t i = collection_index(index, collection_text_length(s));
  const unsigned char *p = (const unsigned char *)s;
  while (i--) {
    ++p;
    while ((*p & 0xc0) == 0x80)
      ++p;
  }
  const unsigned char *end = p + 1;
  while ((*end & 0xc0) == 0x80)
    ++end;
  char *out = collection_alloc((size_t)(end - p) + 1, 1);
  memcpy(out, p, (size_t)(end - p));
  return (uint64_t)(uintptr_t)out;
}

// Single fixed ABI; static typing determines each operation's inputs/results.
uint64_t plenty_collection(int64_t op, uint64_t a, uint64_t b, uint64_t value,
                           const char *descriptor) {
  if (descriptor && *descriptor == 's') {
    const char *text = (const char *)(uintptr_t)a;
    if (op == 5)
      return collection_text_length(text);
    if (op == 4 || op == 6)
      return collection_text_at(text, (int64_t)b);
    if (op == 7)
      return strstr((const char *)(uintptr_t)b, text) != NULL;
    collection_error("invalid string operation");
  }
  PlentyCollection *c = (PlentyCollection *)(uintptr_t)a;
  switch (op) {
  case 0:
    return (uint64_t)(uintptr_t)collection_new(collection_type(&descriptor));
  case 1:
    collection_insert(c, b, value);
    return a;
  case 2:
    c = collection_copy(c);
    collection_insert(c, b, value);
    return (uint64_t)(uintptr_t)c;
  case 3:
    c = collection_copy(c);
    if (c->type->kind == 'L')
      c->entries[collection_index((int64_t)b, c->len)].key = value;
    else
      collection_insert(c, b, value);
    return (uint64_t)(uintptr_t)c;
  case 4:
    if (c->type->kind == 'D') {
      size_t i = collection_find(c, b);
      if (i == SIZE_MAX)
        collection_error("dictionary key not found");
      return c->entries[i].value;
    }
    return collection_at(c, collection_index((int64_t)b, c->len));
  case 5:
    return c->len;
  case 6:
    return collection_at(c, collection_index((int64_t)b, c->len));
  case 7: {
    c = (PlentyCollection *)(uintptr_t)b;
    if (c->type->kind == 'R') {
      __int128 delta = (__int128)(int64_t)a - c->start;
      return c->len && delta % c->step == 0 && delta / c->step >= 0 &&
             delta / c->step < (__int128)c->len;
    }
    if (c->type->kind != 'L')
      return collection_find(c, a) != SIZE_MAX;
    for (size_t i = 0; i < c->len; ++i)
      if (collection_value_equal(a, c->entries[i].key, c->type->key))
        return 1;
    return 0;
  }
  case 8:
    return collection_equal(c, (PlentyCollection *)(uintptr_t)b);
  case 9:
    collection_print(c);
    fputc('\n', stdout);
    return 0;
  case 10: {
    const char *range_type = "R";
    c = collection_new(collection_type(&range_type));
    c->start = (int64_t)a;
    c->stop = (int64_t)b;
    c->step = (int64_t)value;
    if (!c->step)
      collection_error("range step cannot be zero");
    __int128 distance = c->step > 0 ? (__int128)c->stop - c->start
                                    : (__int128)c->start - c->stop;
    __int128 step = c->step > 0 ? (__int128)c->step : -(__int128)c->step;
    __int128 length = distance <= 0 ? 0 : (distance + step - 1) / step;
    if (length > INT64_MAX)
      collection_error("range length exceeds i64");
    c->len = (size_t)length;
    return (uint64_t)(uintptr_t)c;
  }
  case 11: {
    PlentyType *t = collection_alloc(1, sizeof(*t));
    *t = (PlentyType){'L', c->type->value, NULL};
    PlentyCollection *out = collection_new(t);
    for (size_t i = 0; i < c->len; ++i)
      collection_insert(out, c->entries[i].value, 0);
    return (uint64_t)(uintptr_t)out;
  }
  default:
    collection_error("invalid collection operation");
  }
}
