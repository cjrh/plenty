/* LLVM comparison: scalar source, no SIMD intrinsics, no fast-math. */
typedef unsigned int u32;
typedef __SIZE_TYPE__ size_t;
void llvm_u32(const u32 *restrict x, u32 *restrict y, size_t n) {
    for (size_t i = 0; i < n; ++i) y[i] = x[i] * 3u + 7u;
}
void llvm_f32(const float *restrict x, float *restrict y, size_t n) {
    for (size_t i = 0; i < n; ++i) y[i] = x[i] * 3.0f + 7.0f;
}
size_t llvm_count(const u32 *x, size_t n) {
    size_t count = 0;
    for (size_t i = 0; i < n; ++i) count += (x[i] & 1u) == 0;
    return count;
}
float llvm_fold(const float *x, size_t n) {
    float acc = 0.0f;
    for (size_t i = 0; i < n; ++i) acc += x[i];
    return acc;
}
