#include <stdio.h>
__attribute__((noinline)) double windows(const double *xs, long n, long width) {
    double total = 0.0;
    if (width <= 0 || width > n) return total;
    for (long i = 0; i <= n - width; i++)
        for (long j = 0; j < width; j++) total += xs[i + j];
    return total;
}
int main(void) {
    double xs[1024];
    for (long i = 0; i < 1024; i++) xs[i] = i % 16;
    double total = 0.0;
    for (long repeat = 0; repeat < 100000; repeat++) {
        __asm__ volatile("" ::: "memory");
        total += windows(xs, 1024, 4);
    }
    printf("%.1f\n", total);
}
