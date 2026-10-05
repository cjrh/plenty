// Native collection storage. All source-visible collections are immutable
// values. Only compiler-private builders mutate in place. An update returns a
// new value. Objects, entries, buffers, and type metadata have explicit owners.
#include <inttypes.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static _Noreturn void collection_error(const char *message) {
  plenty_value_error(message);
}
static void *collection_alloc(size_t count, size_t size) {
  if (size && count > SIZE_MAX / size) collection_error("collection capacity overflow");
  return plenty_alloc(count * size);
}

typedef struct PlentyType PlentyType;
typedef struct PlentyVariant {
  char *name;
  size_t count;
  PlentyType **fields;
} PlentyVariant;
struct PlentyType {
  PlentyObject object;
  char kind;
  struct PlentyType *key, *value;
  char *name;
  size_t count;
  PlentyVariant *variants;
};
typedef struct PlentyEntry {
  uint64_t key, value;
} PlentyEntry;
typedef struct PlentyCollection {
  PlentyObject object;
  PlentyType *type;
  size_t len, capacity, table_capacity;
  PlentyEntry *entries;
  size_t *table; // entry index + 1, zero denotes an empty bucket
  int64_t start, stop, step;
} PlentyCollection;

typedef struct PlentyEnum {
  PlentyObject object;
  PlentyType *type;
  uint64_t tag;
  uint64_t fields[];
} PlentyEnum;
_Static_assert(offsetof(PlentyEnum, fields) == 32, "enum ABI");

static int collection_managed(const PlentyType *t) {
  return t && (t->kind == 's' || t->kind == 'L' || t->kind == 'S' || t->kind == 'D' || t->kind == 'R' || t->kind == 'E');
}
static void collection_retain_value(uint64_t value, const PlentyType *t) {
  if (collection_managed(t)) plenty_retain((void *)(uintptr_t)value);
}
static void collection_release_value(uint64_t value, const PlentyType *t) {
  if (collection_managed(t)) plenty_release((void *)(uintptr_t)value);
}
static void collection_type_destroy(void *ptr) {
  PlentyType *t = ptr;
  for (size_t i = 0; i < t->count; ++i) {
    PlentyVariant *v = &t->variants[i];
    for (size_t j = 0; j < v->count; ++j) plenty_release(v->fields[j]);
    free(v->fields);
    free(v->name);
  }
  free(t->variants);
  free(t->name);
  plenty_release(t->key);
  plenty_release(t->value);
  free(t);
}
static void collection_destroy(void *ptr) {
  PlentyCollection *c = ptr;
  if (c->type->kind != 'R') {
    for (size_t i = 0; i < c->len; ++i) {
      collection_release_value(c->entries[i].key, c->type->key);
      collection_release_value(c->entries[i].value, c->type->value);
    }
  }
  plenty_release(c->type);
  free(c->entries);
  free(c->table);
  free(c);
}
static size_t descriptor_count(const char **text) {
  size_t n = 0;
  while (**text >= '0' && **text <= '9') n = n * 10 + (size_t)(*(*text)++ - '0');
  if (*(*text)++ != ':') collection_error("invalid type descriptor");
  return n;
}
static char *descriptor_name(const char **text) {
  size_t len = descriptor_count(text);
  char *name = collection_alloc(len + 1, 1);
  memcpy(name, *text, len);
  *text += len;
  return name;
}
typedef struct TypeContext { PlentyType **enums; size_t count; } TypeContext;
static PlentyType *parse_type(const char **text, TypeContext *context) {
  if (**text == '@') {
    ++*text;
    size_t id = descriptor_count(text);
    if (id >= context->count) collection_error("invalid enum type reference");
    plenty_retain(context->enums[id]);
    return context->enums[id];
  }
  PlentyType *t = collection_alloc(1, sizeof(*t));
  t->object = (PlentyObject){1, collection_type_destroy};
  t->kind = *(*text)++;
  if (t->kind == 'L' || t->kind == 'S' || t->kind == 'D')
    t->key = parse_type(text, context);
  if (t->kind == 'D')
    t->value = parse_type(text, context);
  if (t->kind == 'E') {
    PlentyType **entries = collection_alloc(context->count + 1, sizeof(*entries));
    if (context->count) memcpy(entries, context->enums, context->count * sizeof(*entries));
    free(context->enums);
    context->enums = entries;
    context->enums[context->count++] = t;
    t->name = descriptor_name(text);
    t->count = descriptor_count(text);
    t->variants = collection_alloc(t->count, sizeof(*t->variants));
    for (size_t i = 0; i < t->count; ++i) {
      PlentyVariant *v = &t->variants[i];
      v->name = descriptor_name(text);
      v->count = descriptor_count(text);
      v->fields = collection_alloc(v->count, sizeof(*v->fields));
      for (size_t j = 0; j < v->count; ++j) v->fields[j] = parse_type(text, context);
    }
  }
  return t;
}
static PlentyType *collection_type(const char **text) {
  TypeContext context = {0};
  PlentyType *type = parse_type(text, &context);
  free(context.enums);
  return type;
}
static PlentyCollection *collection_new(PlentyType *type) {
  PlentyCollection *c = collection_alloc(1, sizeof(*c));
  c->object = (PlentyObject){1, collection_destroy};
  c->type = type;
  return c;
}
static uint64_t collection_hash(uint64_t value, const PlentyType *t) {
  if (t->kind == 's') {
    uint64_t h = UINT64_C(14695981039346656037);
    const PlentyStr *s = (const PlentyStr *)(uintptr_t)value;
    for (size_t i = 0; i < s->byte_len; ++i) {
      h ^= s->data[i];
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
  if (a == b) return 1;
  if (t->kind == 'E') {
    const PlentyEnum *x = (const PlentyEnum *)(uintptr_t)a, *y = (const PlentyEnum *)(uintptr_t)b;
    if (strcmp(x->type->name, y->type->name) || x->tag != y->tag) return 0;
    const PlentyVariant *v = &t->variants[x->tag];
    for (size_t i = 0; i < v->count; ++i)
      if (!collection_value_equal(x->fields[i], y->fields[i], v->fields[i])) return 0;
    return 1;
  }
  if (t->kind == 's')
    return plenty_str_eq((const PlentyStr *)(uintptr_t)a, (const PlentyStr *)(uintptr_t)b);
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
  free(c->table);
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
    free(c->entries);
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
      collection_retain_value(value, c->type->value);
      collection_release_value(c->entries[existing].value, c->type->value);
      c->entries[existing].value = value;
      return;
    }
  }
  collection_reserve(c);
  collection_retain_value(key, c->type->key);
  collection_retain_value(value, c->type->value);
  c->entries[c->len] = (PlentyEntry){key, value};
  if (c->type->kind != 'L')
    c->table[collection_bucket(c, key)] = c->len + 1;
  ++c->len;
}
static PlentyCollection *collection_copy(const PlentyCollection *source) {
  plenty_retain(source->type);
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
  collection_retain_value(c->entries[i].key, c->type->key);
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
  if (t->kind == 'E') {
    const PlentyEnum *e = (const PlentyEnum *)(uintptr_t)value;
    const PlentyVariant *v = &t->variants[e->tag];
    printf("%s.%s", t->name, v->name);
    if (v->count) {
      fputc('(', stdout);
      for (size_t i = 0; i < v->count; ++i) {
        if (i) fputs(", ", stdout);
        collection_print_value(e->fields[i], v->fields[i]);
      }
      fputc(')', stdout);
    }
  } else if (t->kind >= '1' && t->kind <= '4')
    printf("%" PRId64, (int64_t)value);
  else if (t->kind >= '5' && t->kind <= '8')
    printf("%" PRIu64, value);
  else if (t->kind == 'b')
    fputs(value ? "True" : "False", stdout);
  else if (t->kind == 's') {
    plenty_str_repr((const PlentyStr *)(uintptr_t)value, 0);
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
static uint64_t collection_text_at(const PlentyStr *s, int64_t index) {
  size_t i = collection_index(index, s->scalar_len);
  size_t offset = 0;
  while (i--) {
    ++offset;
    while (offset < s->byte_len && (s->data[offset] & 0xc0) == 0x80) ++offset;
  }
  size_t end = offset + 1;
  while (end < s->byte_len && (s->data[end] & 0xc0) == 0x80) ++end;
  return (uint64_t)(uintptr_t)plenty_str_new(s->data + offset, end - offset);
}

static void enum_destroy(void *ptr) {
  PlentyEnum *e = ptr;
  PlentyVariant *v = &e->type->variants[e->tag];
  for (size_t i = 0; i < v->count; ++i) collection_release_value(e->fields[i], v->fields[i]);
  plenty_release(e->type);
  free(e);
}

static PlentyType *value_type(uint64_t value) {
  // Both objects embed this pointer after PlentyObject. memcpy avoids C
  // effective-type aliasing when the object is an enum rather than a collection.
  PlentyType *type;
  memcpy(&type, (const unsigned char *)(uintptr_t)value + sizeof(PlentyObject), sizeof(type));
  return type;
}

// Single fixed ABI; static typing determines each operation's inputs/results.
uint64_t plenty_collection(int64_t op, uint64_t a, uint64_t b, uint64_t value,
                           const char *descriptor) {
  if (descriptor && *descriptor == 's') {
    const PlentyStr *text = (const PlentyStr *)(uintptr_t)a;
    if (op == 5)
      return text->scalar_len;
    if (op == 12) return text->byte_len;
    if (op == 13) {
      if (b >= text->byte_len || (text->data[b] & 0xc0) == 0x80) collection_error("invalid string cursor");
      size_t end = b + 1;
      while (end < text->byte_len && (text->data[end] & 0xc0) == 0x80) ++end;
      return (uint64_t)(uintptr_t)plenty_str_new(text->data + b, end - b);
    }
    if (op == 4 || op == 6)
      return collection_text_at(text, (int64_t)b);
    if (op == 7)
      return plenty_contains((const PlentyStr *)(uintptr_t)b, text);
    collection_error("invalid string operation");
  }
  PlentyCollection *c = (PlentyCollection *)(uintptr_t)a;
  switch (op) {
  case 0:
    return (uint64_t)(uintptr_t)collection_new(collection_type(&descriptor));
  case 1:
    collection_insert(c, b, value);
    plenty_retain(c);
    return a;
  case 2:
    c = collection_copy(c);
    collection_insert(c, b, value);
    return (uint64_t)(uintptr_t)c;
  case 3:
    c = collection_copy(c);
    if (c->type->kind == 'L') {
      size_t i = collection_index((int64_t)b, c->len);
      collection_retain_value(value, c->type->key);
      collection_release_value(c->entries[i].key, c->type->key);
      c->entries[i].key = value;
    } else
      collection_insert(c, b, value);
    return (uint64_t)(uintptr_t)c;
  case 4:
    if (c->type->kind == 'D') {
      size_t i = collection_find(c, b);
      if (i == SIZE_MAX)
        collection_error("dictionary key not found");
      collection_retain_value(c->entries[i].value, c->type->value);
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
    return collection_value_equal(a, b, value_type(a));
  case 9:
    collection_print_value(a, value_type(a));
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
    *t = (PlentyType){.object = {1, collection_type_destroy}, .kind = 'L', .key = c->type->value};
    plenty_retain(t->key);
    PlentyCollection *out = collection_new(t);
    for (size_t i = 0; i < c->len; ++i)
      collection_insert(out, c->entries[i].value, 0);
    return (uint64_t)(uintptr_t)out;
  }
  case 20: {
    PlentyType *t = collection_type(&descriptor);
    if (t->kind != 'E' || a >= t->count) collection_error("invalid enum variant");
    PlentyEnum *e = collection_alloc(1, sizeof(*e) + t->variants[a].count * sizeof(uint64_t));
    e->object = (PlentyObject){1, enum_destroy};
    e->type = t;
    e->tag = a;
    return (uint64_t)(uintptr_t)e;
  }
  case 21: {
    PlentyEnum *e = (PlentyEnum *)(uintptr_t)a;
    PlentyVariant *v = &e->type->variants[e->tag];
    if (b >= v->count) collection_error("invalid enum field");
    collection_retain_value(value, v->fields[b]);
    collection_release_value(e->fields[b], v->fields[b]);
    e->fields[b] = value;
    return 0;
  }
  case 22:
    return ((PlentyEnum *)(uintptr_t)a)->tag;
  case 23: {
    PlentyEnum *e = (PlentyEnum *)(uintptr_t)a;
    PlentyVariant *v = &e->type->variants[e->tag];
    if (e->tag != value || b >= v->count) collection_error("invalid enum projection");
    collection_retain_value(e->fields[b], v->fields[b]);
    return e->fields[b];
  }
  case 24: {
    uint64_t item = 0;
    uint8_t ready = plenty_generator_resume((PlentyGenerator *)(uintptr_t)a, &item);
    uint64_t option = plenty_collection(20, ready ? 1 : 0, 0, 0, descriptor);
    if (ready) {
      plenty_collection(21, option, 0, item, NULL);
      PlentyEnum *e = (PlentyEnum *)(uintptr_t)option;
      collection_release_value(item, e->type->variants[1].fields[0]);
    }
    return option;
  }
  default:
    collection_error("invalid collection operation");
  }
}
