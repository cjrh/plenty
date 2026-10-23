#include <assert.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

void clif_u32_1(const uint32_t *, uint32_t *, size_t);
void clif_u32_4(const uint32_t *, uint32_t *, size_t);
void llvm_u32(const uint32_t *, uint32_t *, size_t);
void clif_f32_1(const float *, float *, size_t);
void clif_f32_4(const float *, float *, size_t);
void llvm_f32(const float *, float *, size_t);
size_t llvm_count(const uint32_t *, size_t);
float llvm_fold(const float *, size_t);

/* A caller-side scalar tail models the proposed lowering's bounds contract. */
static void vector_u32(const uint32_t *x, uint32_t *y, size_t n) {
    size_t full = n - n % 4;
    clif_u32_4(x, y, full);
    clif_u32_1(x + full, y + full, n - full);
}
static void vector_f32(const float *x, float *y, size_t n) {
    size_t full = n - n % 4;
    clif_f32_4(x, y, full);
    clif_f32_1(x + full, y + full, n - full);
}
static double now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return t.tv_sec + t.tv_nsec * 1e-9;
}
int main(void) {
    const size_t capacity = 262144;
    uint32_t *storage = malloc((capacity + 1) * sizeof(uint32_t));
    uint32_t *out = malloc((capacity + 1) * sizeof(uint32_t));
    uint32_t *expected = malloc((capacity + 1) * sizeof(uint32_t));
    assert(storage && out && expected);
    /* Offset by one scalar so SIMD cannot rely on vector alignment. */
    uint32_t *x = storage + 1;
    for (size_t i = 0; i < capacity; ++i) x[i] = (uint32_t)i * 2654435761u;
    for (size_t n = 0; n < 130; ++n) {
        clif_u32_1(x, expected, n);
        out[n] = 0xdeadbeef;
        vector_u32(x, out, n);
        assert(memcmp(expected, out, n * sizeof(uint32_t)) == 0);
        assert(out[n] == 0xdeadbeef);
        llvm_u32(x, out, n);
        assert(memcmp(expected, out, n * sizeof(uint32_t)) == 0);
        size_t count = 0;
        for (size_t i = 0; i < n; ++i) count += (x[i] & 1u) == 0;
        assert(llvm_count(x, n) == count);
    }
    float fs[] = {0.0f, -0.0f, INFINITY, -INFINITY, NAN, 0x1p-149f, -0x1p-149f, 1.5f, -3.25f};
    float a[9], b[9], c[9];
    for (size_t n = 0; n <= 9; ++n) {
        clif_f32_1(fs, a, n);
        vector_f32(fs, b, n);
        llvm_f32(fs, c, n);
        for (size_t i = 0; i < n; ++i) {
            assert((isnan(a[i]) && isnan(b[i]) && isnan(c[i])) ||
                   (memcmp(a + i, b + i, sizeof(float)) == 0 &&
                    memcmp(a + i, c + i, sizeof(float)) == 0));
        }
    }
    float cancellation[] = {1e20f, 1.0f, -1e20f, 1.0f};
    assert(llvm_fold(cancellation, 4) == 1.0f);
    float regrouped = (cancellation[0] + cancellation[2]) + (cancellation[1] + cancellation[3]);
    printf("checks passed; ordered float fold=1, regrouped=%g\n", regrouped);
    void (*kernels[])(const uint32_t *, uint32_t *, size_t) = {clif_u32_1, vector_u32, llvm_u32};
    const char *names[] = {"Cranelift scalar", "Cranelift 128+tail", "LLVM scalar source"};
    const size_t sizes[] = {7, 1024, capacity};
    volatile uint32_t sink = 0;
    for (size_t s = 0; s < 3; ++s) {
        size_t n = sizes[s], repeats = 16777216 / n;
        for (size_t k = 0; k < 3; ++k) {
            double best = 1e9;
            for (int trial = 0; trial < 5; ++trial) {
                double start = now();
                for (size_t r = 0; r < repeats; ++r) {
                    kernels[k](x, out, n);
                    sink ^= out[r % n];
                }
                double elapsed = now() - start;
                if (elapsed < best) best = elapsed;
            }
            printf("n=%zu %s %.3f ns/item\n", n, names[k], best * 1e9 / (n * repeats));
        }
    }
    (void)sink;
    free(storage); free(out); free(expected);
}
