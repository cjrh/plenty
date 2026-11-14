#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
typedef struct { double x, y, z; } Vec3;
Vec3 advance(Vec3 value, const Vec3 *delta, double factor, int64_t steps);
int main(int argc, char **argv) {
    int64_t steps = argc > 1 ? strtoll(argv[1], 0, 10) : 10000;
    Vec3 delta = { 0.0001, -0.0002, 0.0003 };
    Vec3 result = advance((Vec3){1.0, 2.0, 3.0}, &delta, 0.9999995, steps);
    printf("%.17g %.17g %.17g\n", result.x, result.y, result.z);
    return 0;
}
