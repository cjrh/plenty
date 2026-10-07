#include <stdint.h>

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
