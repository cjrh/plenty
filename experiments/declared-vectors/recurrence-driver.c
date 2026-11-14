#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
void recurrence(double *, const double *, double, int64_t);
int main(int argc, char **argv) {
    int64_t steps = argc > 1 ? strtoll(argv[1], 0, 10) : 10000;
    double state[] = { 1.0, 2.0, 3.0 };
    double delta[] = { 0.0001, -0.0002, 0.0003 };
    recurrence(state, delta, 0.9999995, steps);
    printf("%.17g %.17g %.17g\n", state[0], state[1], state[2]);
    return 0;
}
