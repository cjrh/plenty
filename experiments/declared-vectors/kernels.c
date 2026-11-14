#include <stdint.h>
typedef struct { double x, y, z; } Vec3;

Vec3 advance(Vec3 value, const Vec3 *delta, double factor, int64_t steps) {
    for (int64_t i = 0; i < steps; ++i) {
        value.x = value.x * factor + delta->x;
        value.y = value.y * factor + delta->y;
        value.z = value.z * factor + delta->z;
    }
    return value;
}
