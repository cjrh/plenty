#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
extern double projection(double, double, int64_t);
int main(int argc, char **argv) {
    int64_t n = argc > 1 ? strtoll(argv[1], NULL, 10) : 1000;
    printf("%.17g\n", projection(1.0, 2.0, n));
}
