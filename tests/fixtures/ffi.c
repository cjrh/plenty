#include <stdint.h>
#include <stdlib.h>
#include <string.h>

int64_t fixture_narrow(int8_t a, uint8_t b, int16_t c, uint16_t d,
                       int32_t e, uint32_t f, int64_t g, uint64_t h) {
    return a + (int64_t)b + c + d + e + (int64_t)f + g + (int64_t)h;
}
int8_t fixture_small(int8_t value) { return value; }
uint64_t fixture_unsigned(uint64_t value) { return value; }
double fixture_real(float a, double b) { return a + b; }
float fixture_single(float value) { return value; }
static int64_t token = 42;
void *fixture_token(void) { return &token; }
int64_t fixture_value(const void *value) { return *(const int64_t *)value; }
void fixture_reset(void) { token = 42; }

static int64_t live = 0, opened = 0, text_calls = 0;
int64_t fixture_live(void) { return live; }
int64_t fixture_opened(void) { return opened; }
int32_t fixture_open(void **out, int32_t mode) {
    ++opened;
    *out = NULL;
    if (mode == 1) return 1;
    int64_t *value = malloc(sizeof(*value));
    if (!value) return 1;
    *value = 42;
    *out = value;
    ++live;
    return mode == 2 ? 2 : 0;
}
void fixture_close(void *value) { if (value) { --live; free(value); } }
void fixture_set(void *value, int64_t n) { *(int64_t *)value = n; }
int32_t fixture_transfer(void *value, int32_t fail) {
    if (fail) return fail;
    fixture_close(value);
    return 0;
}
int32_t fixture_consume(void *value, int32_t fail) {
    fixture_close(value);
    return fail;
}
void fixture_outputs(int8_t *a, uint16_t *b, float *c, double *d, int64_t *e) {
    *a = -128; *b = 65535; *c = 1.5f; *d = -2.25; *e = INT64_MIN;
}
int64_t fixture_read(const int64_t *value) { return *value; }
uint64_t fixture_bytes(const uint8_t *bytes, size_t count) {
    uint64_t sum = 0;
    for (size_t i = 0; i < count; ++i) sum += bytes[i];
    return sum;
}
uint64_t fixture_strings(const char *a, const char *b) {
    ++text_calls;
    return strlen(a) + strlen(b);
}
void fixture_string_void(const char *text) { text_calls += (int64_t)strlen(text); }
int64_t fixture_text_calls(void) { return text_calls; }
float fixture_string_float(const char *text) { return (float)strlen(text) + 0.5f; }
void *fixture_string_pointer(const char *text) { (void)text; return &token; }
