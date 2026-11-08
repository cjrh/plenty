// Issue #655: does block bitpacking speed up clist column scans?
//
// Compares `count(v > t)` over one u32 column stored natively (scalar and
// SSE2, matching the two paths Plenty emits today), as narrower native
// columns (u8/u16 SSE2, a path Plenty does not vectorize yet), and as
// simdcomp-style 4x-interleaved frame-of-reference bitpacked blocks of 128
// values, optionally delta-coded. It also times the codec and the mutation
// operations a mutable clist would need. Baseline x86-64 SSE2 only, like
// Plenty's current target policy.
#define _GNU_SOURCE
#include <emmintrin.h>
#include <linux/perf_event.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

typedef uint8_t u8;
typedef uint16_t u16;
typedef uint32_t u32;
typedef uint64_t u64;

#define BLOCK 128

static int perf_fd = -1;

static void perf_open(void) {
    struct perf_event_attr attr;
    memset(&attr, 0, sizeof attr);
    attr.type = PERF_TYPE_HARDWARE;
    attr.size = sizeof attr;
    attr.config = PERF_COUNT_HW_INSTRUCTIONS;
    attr.disabled = 1;
    attr.exclude_kernel = 1;
    attr.exclude_hv = 1;
    perf_fd = (int)syscall(SYS_perf_event_open, &attr, 0, -1, -1, 0);
}

static u64 perf_read(void) {
    u64 v = 0;
    if (perf_fd >= 0 && read(perf_fd, &v, sizeof v) != sizeof v) v = 0;
    return v;
}

static double now(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec + ts.tv_nsec * 1e-9;
}

static u64 rng = 0x9E3779B97F4A7C15ull;
static u32 next_u32(void) {
    rng ^= rng << 13;
    rng ^= rng >> 7;
    rng ^= rng << 17;
    return (u32)(rng >> 16);
}

static void *xalloc(size_t n) {
    void *p = aligned_alloc(64, (n + 63) & ~(size_t)63);
    if (!p) {
        perror("alloc");
        exit(1);
    }
    return p;
}

// Keeps results observable so the compiler cannot drop a measured scan.
static volatile u64 sink;

// ---------------------------------------------------------------- native

__attribute__((noipa)) static u64 native_scalar(const u32 *v, size_t n, u32 t) {
    u64 c = 0;
    for (size_t i = 0; i < n; i++) c += v[i] > t;
    return c;
}

static inline __m128i bias32(__m128i x) { return _mm_xor_si128(x, _mm_set1_epi32((int)0x80000000u)); }

// Same shape as Plenty's vector loop: four lanes, 64-bit accumulation.
__attribute__((noipa)) static u64 native_sse2(const u32 *v, size_t n, u32 t) {
    __m128i tv = bias32(_mm_set1_epi32((int)t));
    u64 c = 0;
    size_t i = 0;
    while (i + 4 <= n) {
        // 32-bit lane counters cannot overflow within 2^30 groups.
        size_t end = i + ((n - i) / 4 < (1u << 30) ? (n - i) / 4 * 4 : (size_t)4 << 30);
        __m128i acc = _mm_setzero_si128();
        for (; i < end; i += 4) {
            __m128i x = bias32(_mm_loadu_si128((const __m128i *)(v + i)));
            acc = _mm_sub_epi32(acc, _mm_cmpgt_epi32(x, tv));
        }
        u32 lanes[4];
        _mm_storeu_si128((__m128i *)lanes, acc);
        c += (u64)lanes[0] + lanes[1] + lanes[2] + lanes[3];
    }
    for (; i < n; i++) c += v[i] > t;
    return c;
}

__attribute__((noipa)) static u64 native_u16_sse2(const u16 *v, size_t n, u16 t) {
    const __m128i b = _mm_set1_epi16((short)0x8000);
    __m128i tv = _mm_xor_si128(_mm_set1_epi16((short)t), b);
    const __m128i ones = _mm_set1_epi16(1);
    u64 c = 0;
    size_t i = 0;
    while (i + 8 <= n) {
        size_t groups = (n - i) / 8;
        if (groups > 32767) groups = 32767;
        size_t end = i + groups * 8;
        __m128i acc = _mm_setzero_si128();
        for (; i < end; i += 8) {
            __m128i x = _mm_xor_si128(_mm_loadu_si128((const __m128i *)(v + i)), b);
            acc = _mm_sub_epi16(acc, _mm_cmpgt_epi16(x, tv));
        }
        u32 lanes[4];
        _mm_storeu_si128((__m128i *)lanes, _mm_madd_epi16(acc, ones));
        c += (u64)lanes[0] + lanes[1] + lanes[2] + lanes[3];
    }
    for (; i < n; i++) c += v[i] > t;
    return c;
}

__attribute__((noipa)) static u64 native_u8_sse2(const u8 *v, size_t n, u8 t) {
    const __m128i b = _mm_set1_epi8((char)0x80);
    __m128i tv = _mm_xor_si128(_mm_set1_epi8((char)t), b);
    u64 c = 0;
    size_t i = 0;
    while (i + 16 <= n) {
        size_t groups = (n - i) / 16;
        if (groups > 255) groups = 255;
        size_t end = i + groups * 16;
        __m128i acc = _mm_setzero_si128();
        for (; i < end; i += 16) {
            __m128i x = _mm_xor_si128(_mm_loadu_si128((const __m128i *)(v + i)), b);
            acc = _mm_sub_epi8(acc, _mm_cmpgt_epi8(x, tv));
        }
        __m128i s = _mm_sad_epu8(acc, _mm_setzero_si128());
        c += (u64)_mm_cvtsi128_si32(s) + (u64)_mm_cvtsi128_si32(_mm_srli_si128(s, 8));
    }
    for (; i < n; i++) c += v[i] > t;
    return c;
}

// Native column plus a per-block min/max zone map: isolates block skipping
// from bitpacking.
typedef struct {
    u32 *min, *max;
} ZoneMap;

__attribute__((noipa)) static u64 native_zonemap(const u32 *v, const ZoneMap *z, size_t n, u32 t) {
    u64 c = 0;
    for (size_t b = 0; b < n / BLOCK; b++) {
        if (z->min[b] > t) {
            c += BLOCK;
        } else if (z->max[b] > t) {
            c += native_sse2(v + b * BLOCK, BLOCK, t);
        }
    }
    return c;
}

// ------------------------------------------------------------ bitpacked
//
// Block layout (simdcomp's 4x "SIMD" layout): value i of a block lives in
// lane i % 4 at position i / 4. Each lane packs its 32 values with `bits`
// bits; word k of lane l is u32 index 4k + l. A block is `bits` vectors.
// Frame of reference: stored value = value - block min.

typedef struct {
    size_t n, nblocks;
    u8 *bits;
    u32 *min, *max; // max is the true block max (zone map)
    u32 *offset;    // block start, in 16-byte vectors
    __m128i *data;
    __m128i *base; // delta mode: the 4 values preceding each block (block's own)
    int delta;
} Packed;

static int width_of(u32 range) {
    int b = 0;
    while (b < 32 && (range >> b) != 0) b++;
    return b;
}

static void pack_block(const u32 *in, int bits, __m128i *out) {
    u32 *w = (u32 *)out;
    memset(w, 0, (size_t)bits * 16);
    for (int i = 0; i < BLOCK; i++) {
        int lane = i & 3, j = i >> 2;
        u64 bitpos = (u64)j * bits;
        u32 word = (u32)(bitpos >> 5), off = (u32)(bitpos & 31);
        u64 val = in[i];
        w[word * 4 + lane] |= (u32)(val << off);
        if (off + bits > 32) w[(word + 1) * 4 + lane] |= (u32)(val >> (32 - off));
    }
}

// Encodes `v` into `p`; `tmp` holds the FOR (or delta) residuals of one block.
static void pack_column(Packed *p, const u32 *v, size_t n, int delta) {
    p->n = n;
    p->nblocks = n / BLOCK;
    p->delta = delta;
    u32 tmp[BLOCK];
    size_t total = 0;
    for (size_t b = 0; b < p->nblocks; b++) {
        const u32 *x = v + b * BLOCK;
        u32 mn = x[0], mx = x[0];
        for (int i = 1; i < BLOCK; i++) {
            if (x[i] < mn) mn = x[i];
            if (x[i] > mx) mx = x[i];
        }
        p->min[b] = mn;
        p->max[b] = mx;
        u32 range;
        if (delta) {
            // D4 delta: residual i = v[i] - v[i-4], the block's first four
            // values relative to its own base vector (so blocks decode
            // independently). Requires sorted input.
            u32 *bs = (u32 *)&p->base[b];
            for (int l = 0; l < 4; l++) bs[l] = x[0];
            range = 0;
            for (int i = 0; i < BLOCK; i++) {
                u32 prev = i < 4 ? x[0] : x[i - 4];
                tmp[i] = x[i] - prev;
                if (tmp[i] > range) range = tmp[i];
            }
        } else {
            for (int i = 0; i < BLOCK; i++) tmp[i] = x[i] - mn;
            range = mx - mn;
        }
        int bits = width_of(range);
        p->bits[b] = (u8)bits;
        p->offset[b] = (u32)total;
        pack_block(tmp, bits, p->data + total);
        total += (size_t)bits;
    }
}

static size_t packed_bytes(const Packed *p) {
    size_t vecs = 0;
    for (size_t b = 0; b < p->nblocks; b++) vecs += p->bits[b];
    // Data plus bits (1), min (4), offset (4) per block; delta adds a base
    // vector. The zone-map max is reported separately.
    return vecs * 16 + p->nblocks * (1 + 4 + 4 + (p->delta ? 16 : 0));
}

// Fused unpack + compare for one block at a compile-time width. Returns a
// vector of per-lane match counts (each at most 32).
#define UNPACK_BODY(B, EMIT)                                                     \
    const __m128i mask = _mm_set1_epi32((int)((B) == 32 ? 0xFFFFFFFFu : ((1u << (B)) - 1))); \
    __m128i w = _mm_loadu_si128(in);                                             \
    int shift = 0, k = 0;                                                        \
    _Pragma("GCC unroll 32") for (int j = 0; j < 32; j++) {                      \
        __m128i x = _mm_srli_epi32(w, shift);                                    \
        if (shift + (B) > 32) {                                                  \
            k++;                                                                 \
            w = _mm_loadu_si128(in + k);                                         \
            x = _mm_or_si128(x, _mm_slli_epi32(w, 32 - shift));                  \
            shift = shift + (B) - 32;                                            \
        } else {                                                                 \
            shift += (B);                                                        \
            if (shift == 32 && j < 31) {                                         \
                k++;                                                             \
                w = _mm_loadu_si128(in + k);                                     \
                shift = 0;                                                       \
            }                                                                    \
        }                                                                        \
        if ((B) < 32) x = _mm_and_si128(x, mask);                                \
        EMIT                                                                     \
    }

#define DEFINE_WIDTH(B)                                                                    \
    static __m128i count_for_##B(const __m128i *in, __m128i t, __m128i base) {             \
        (void)base;                                                                        \
        __m128i acc = _mm_setzero_si128();                                                 \
        UNPACK_BODY(B, acc = _mm_sub_epi32(acc, _mm_cmpgt_epi32(x, t));)                   \
        return acc;                                                                        \
    }                                                                                      \
    static __m128i count_full_##B(const __m128i *in, __m128i t, __m128i base) {           \
        __m128i acc = _mm_setzero_si128();                                                 \
        UNPACK_BODY(B, acc = _mm_sub_epi32(acc, _mm_cmpgt_epi32(_mm_add_epi32(x, base), t));) \
        return acc;                                                                        \
    }                                                                                      \
    static __m128i count_delta_##B(const __m128i *in, __m128i t, __m128i base) {           \
        __m128i acc = _mm_setzero_si128(), run = base;                                     \
        UNPACK_BODY(B, run = _mm_add_epi32(run, x);                                        \
                    acc = _mm_sub_epi32(acc, _mm_cmpgt_epi32(bias32(run), t));)            \
        return acc;                                                                        \
    }                                                                                      \
    static void unpack_##B(const __m128i *in, u32 *out) {                                  \
        UNPACK_BODY(B, _mm_storeu_si128((__m128i *)(out + 4 * j), x);)                     \
    }

#define WIDTHS(X) X(1) X(2) X(3) X(4) X(5) X(6) X(7) X(8) X(9) X(10) X(11) X(12) X(13) X(14) \
    X(15) X(16) X(17) X(18) X(19) X(20) X(21) X(22) X(23) X(24) X(25) X(26) X(27) X(28) X(29) \
    X(30) X(31) X(32)
WIDTHS(DEFINE_WIDTH)

static __m128i count_zero(const __m128i *in, __m128i t, __m128i base) {
    (void)in;
    // Width 0: every residual is 0.
    return _mm_sub_epi32(_mm_setzero_si128(), _mm_cmpgt_epi32(_mm_setzero_si128(), t));
    (void)base;
}
static __m128i count_delta_zero(const __m128i *in, __m128i t, __m128i base) {
    (void)in;
    __m128i m = _mm_cmpgt_epi32(bias32(base), t);
    return _mm_slli_epi32(_mm_sub_epi32(_mm_setzero_si128(), m), 5);
}
static __m128i count_full_zero(const __m128i *in, __m128i t, __m128i base) {
    (void)in;
    __m128i m = _mm_cmpgt_epi32(base, t);
    return _mm_slli_epi32(_mm_sub_epi32(_mm_setzero_si128(), m), 5);
}
static void unpack_zero(const __m128i *in, u32 *out) {
    (void)in;
    memset(out, 0, BLOCK * 4);
}

typedef __m128i (*CountFn)(const __m128i *, __m128i, __m128i);
typedef void (*UnpackFn)(const __m128i *, u32 *);
#define FOR_ENTRY(B) count_for_##B,
#define DELTA_ENTRY(B) count_delta_##B,
#define FULL_ENTRY(B) count_full_##B,
#define UNPACK_ENTRY(B) unpack_##B,
static const CountFn count_for[33] = {count_zero, WIDTHS(FOR_ENTRY)};
static const CountFn count_full[33] = {count_full_zero, WIDTHS(FULL_ENTRY)};
static const CountFn count_delta[33] = {count_delta_zero, WIDTHS(DELTA_ENTRY)};
static const UnpackFn unpack_fn[33] = {unpack_zero, WIDTHS(UNPACK_ENTRY)};

static u64 hsum(__m128i acc) {
    u32 l[4];
    _mm_storeu_si128((__m128i *)l, acc);
    return (u64)l[0] + l[1] + l[2] + l[3];
}

// FOR scan. With `skip`, the metadata needed to decode (min and width)
// settles whole blocks and the rest compare residuals; without it every value
// is reconstructed (residual + min) and compared, the cost of a scan whose
// predicate cannot be rewritten per block.
__attribute__((noipa)) static u64 packed_for_scan(const Packed *p, u32 t, int skip) {
    u64 c = 0;
    if (!skip) {
        __m128i tv = bias32(_mm_set1_epi32((int)t));
        for (size_t b = 0; b < p->nblocks; b++) {
            // Adding the biased min yields the biased value directly.
            __m128i mv = _mm_set1_epi32((int)(p->min[b] ^ 0x80000000u));
            c += hsum(count_full[p->bits[b]](p->data + p->offset[b], tv, mv));
        }
        return c;
    }
    for (size_t b = 0; b < p->nblocks; b++) {
        u32 mn = p->min[b];
        int bits = p->bits[b];
        if (t < mn) {
            c += BLOCK;
            continue;
        }
        u64 hi = (u64)mn + (bits == 32 ? 0xFFFFFFFFull : ((1ull << bits) - 1));
        if ((u64)t >= hi) continue;
        // Here t - mn < 2^bits - 1 < 2^31, so a signed compare is exact.
        u32 rel = t - mn;
        c += hsum(count_for[bits](p->data + p->offset[b], _mm_set1_epi32((int)rel), _mm_setzero_si128()));
    }
    return c;
}

__attribute__((noipa)) static u64 packed_delta_scan(const Packed *p, u32 t, int skip) {
    u64 c = 0;
    __m128i tv = bias32(_mm_set1_epi32((int)t));
    for (size_t b = 0; b < p->nblocks; b++) {
        if (skip) {
            if (p->min[b] > t) {
                c += BLOCK;
                continue;
            }
            if (p->max[b] <= t) continue;
        }
        c += hsum(count_delta[p->bits[b]](p->data + p->offset[b], tv, p->base[b]));
    }
    return c;
}

static void unpack_block(const Packed *p, size_t b, u32 *out) {
    unpack_fn[p->bits[b]](p->data + p->offset[b], out);
    if (p->delta) {
        u32 run[4];
        memcpy(run, &p->base[b], 16);
        for (int i = 0; i < BLOCK; i++) out[i] = run[i & 3] += out[i];
    } else {
        u32 mn = p->min[b];
        for (int i = 0; i < BLOCK; i++) out[i] += mn;
    }
}

__attribute__((noipa)) static void unpack_column(const Packed *p, u32 *out) {
    for (size_t b = 0; b < p->nblocks; b++) unpack_block(p, b, out + b * BLOCK);
}

// Random-access read of one FOR value.
static inline u32 packed_get(const Packed *p, size_t i) {
    size_t b = i / BLOCK;
    u32 r = (u32)(i % BLOCK), lane = r & 3, j = r >> 2;
    u32 bits = p->bits[b];
    if (bits == 0) return p->min[b];
    const u32 *w = (const u32 *)(p->data + p->offset[b]);
    u32 bitpos = j * bits, word = bitpos >> 5, off = bitpos & 31;
    u64 lo = w[word * 4 + lane] >> off;
    if (off + bits > 32) lo |= (u64)w[(word + 1) * 4 + lane] << (32 - off);
    u32 mask = bits == 32 ? 0xFFFFFFFFu : ((1u << bits) - 1);
    return ((u32)lo & mask) + p->min[b];
}

// In-place FOR write when the new residual fits the block's width.
static inline int packed_set_in_place(Packed *p, size_t i, u32 v) {
    size_t b = i / BLOCK;
    u32 bits = p->bits[b], mn = p->min[b];
    if (v < mn) return 0;
    u64 rel = (u64)v - mn;
    if (bits < 32 && rel >> bits) return 0;
    if (bits == 0) return 1;
    u32 r = (u32)(i % BLOCK), lane = r & 3, j = r >> 2;
    u32 *w = (u32 *)(p->data + p->offset[b]);
    u32 bitpos = j * bits, word = bitpos >> 5, off = bitpos & 31;
    u64 mask = bits == 32 ? 0xFFFFFFFFull : ((1ull << bits) - 1);
    u64 cur = w[word * 4 + lane];
    if (off + bits > 32) cur |= (u64)w[(word + 1) * 4 + lane] << 32;
    cur = (cur & ~(mask << off)) | (rel << off);
    w[word * 4 + lane] = (u32)cur;
    if (off + bits > 32) w[(word + 1) * 4 + lane] = (u32)(cur >> 32);
    return 1;
}

// ------------------------------------------------------------- harness

typedef struct {
    double ns_per_row;
    double instr_per_row;
    u64 result;
} Sample;

#define MEASURE(rows_per_rep, reps, EXPR)                                            \
    ({                                                                               \
        Sample best = {1e30, 0, 0};                                                  \
        for (int trial = 0; trial < 5; trial++) {                                    \
            ioctl(perf_fd, PERF_EVENT_IOC_RESET, 0);                                 \
            ioctl(perf_fd, PERF_EVENT_IOC_ENABLE, 0);                                \
            double t0 = now();                                                       \
            u64 acc = 0;                                                             \
            for (size_t rep = 0; rep < (reps); rep++) acc += (EXPR);                 \
            double t1 = now();                                                       \
            ioctl(perf_fd, PERF_EVENT_IOC_DISABLE, 0);                               \
            u64 ins = perf_read();                                                   \
            double rows = (double)(rows_per_rep) * (double)(reps);                   \
            double ns = (t1 - t0) * 1e9 / rows;                                      \
            if (ns < best.ns_per_row) {                                              \
                best.ns_per_row = ns;                                                \
                best.instr_per_row = ins / rows;                                     \
            }                                                                        \
            best.result = acc / (reps);                                              \
        }                                                                            \
        sink += best.result;                                                         \
        best;                                                                        \
    })

static void emit(const char *dist, size_t n, const char *sel, const char *kernel, double bytes_per_row,
                 Sample s, u64 expect) {
    if (s.result != expect) {
        fprintf(stderr, "MISMATCH %s %zu %s %s: %llu != %llu\n", dist, n, sel, kernel,
                (unsigned long long)s.result, (unsigned long long)expect);
        exit(2);
    }
    printf("{\"group\":\"scan\",\"dist\":\"%s\",\"rows\":%zu,\"selectivity\":\"%s\",\"kernel\":\"%s\","
           "\"bytes_per_row\":%.4f,\"ns_per_row\":%.4f,\"instr_per_row\":%.3f}\n",
           dist, n, sel, kernel, bytes_per_row, s.ns_per_row, s.instr_per_row);
    fflush(stdout);
}

static void emit_op(const char *dist, size_t n, const char *op, const char *unit, Sample s) {
    printf("{\"group\":\"op\",\"dist\":\"%s\",\"rows\":%zu,\"op\":\"%s\",\"unit\":\"%s\","
           "\"ns\":%.4f,\"instr\":%.3f}\n",
           dist, n, op, unit, s.ns_per_row, s.instr_per_row);
    fflush(stdout);
}

typedef enum { UNIFORM, SORTED, OUTLIERS } Dist;

// Fills v; returns the largest value so the caller can pick thresholds.
static u32 generate(u32 *v, size_t n, Dist d, int bits) {
    u32 mask = bits == 32 ? 0xFFFFFFFFu : ((1u << bits) - 1), top = 0;
    u32 cur = 1000;
    for (size_t i = 0; i < n; i++) {
        switch (d) {
        case UNIFORM: v[i] = next_u32() & mask; break;
        case SORTED: cur += next_u32() & mask; v[i] = cur; break;
        case OUTLIERS:
            // 1% of rows are wide; with 128-row blocks about 72% of blocks
            // contain at least one.
            v[i] = (next_u32() % 100 == 0) ? (next_u32() & 0xFFFFF) : (next_u32() & mask);
            break;
        }
        if (v[i] > top) top = v[i];
    }
    return top;
}

static u32 quantile(const u32 *v, size_t n, double q) {
    // Approximate: sample 4096 values.
    u32 s[4096];
    for (int i = 0; i < 4096; i++) s[i] = v[(size_t)i * (n / 4096)];
    for (int i = 1; i < 4096; i++) {
        u32 x = s[i];
        int j = i - 1;
        while (j >= 0 && s[j] > x) { s[j + 1] = s[j]; j--; }
        s[j + 1] = x;
    }
    return s[(int)(q * 4095)];
}

static size_t reps_for(size_t n) {
    size_t r = (size_t)400000000 / n;
    return r < 2 ? 2 : r;
}

static void run_case(const char *dist_name, Dist d, int bits, size_t n, int with_ops) {
    u32 *v = xalloc(n * 4);
    u32 top = generate(v, n, d, bits);
    (void)top;
    Packed p = {0};
    size_t nb = n / BLOCK;
    p.bits = xalloc(nb);
    p.min = xalloc(nb * 4);
    p.max = xalloc(nb * 4);
    p.offset = xalloc(nb * 4);
    p.data = xalloc(nb * 32 * 16 + 64);
    p.base = xalloc(nb * 16);
    int delta = d == SORTED;
    pack_column(&p, v, n, 0);
    Packed pd = p;
    if (delta) {
        pd.bits = xalloc(nb);
        pd.min = xalloc(nb * 4);
        pd.max = xalloc(nb * 4);
        pd.offset = xalloc(nb * 4);
        pd.data = xalloc(nb * 32 * 16 + 64);
        pd.base = xalloc(nb * 16);
        pack_column(&pd, v, n, 1);
    }
    ZoneMap z = {p.min, p.max};

    // Narrow native copies exist only when the values fit.
    u8 *v8 = NULL;
    u16 *v16 = NULL;
    if (d == UNIFORM && bits <= 8) {
        v8 = xalloc(n);
        for (size_t i = 0; i < n; i++) v8[i] = (u8)v[i];
    }
    if (d == UNIFORM && bits <= 16) {
        v16 = xalloc(n * 2);
        for (size_t i = 0; i < n; i++) v16[i] = (u16)v[i];
    }

    // Roundtrip check of the codec.
    {
        u32 *back = xalloc(n * 4);
        unpack_column(&p, back);
        if (memcmp(back, v, n * 4)) { fprintf(stderr, "FOR roundtrip failed\n"); exit(2); }
        if (delta) {
            unpack_column(&pd, back);
            if (memcmp(back, v, n * 4)) { fprintf(stderr, "delta roundtrip failed\n"); exit(2); }
        }
        for (size_t i = 0; i < n; i += 997)
            if (packed_get(&p, i) != v[i]) { fprintf(stderr, "get failed\n"); exit(2); }
        free(back);
    }

    const double qs[] = {0.99, 0.5, 0.01};
    const char *qn[] = {"1%", "50%", "99%"}; // fraction of rows matching v > t
    size_t reps = reps_for(n);
    for (int qi = 0; qi < 3; qi++) {
        u32 t = quantile(v, n, qs[qi]);
        u64 expect = native_scalar(v, n, t);
        char dist[64];
        snprintf(dist, sizeof dist, "%s-%d", dist_name, bits);
        emit(dist, n, qn[qi], "native-u32-scalar", 4, MEASURE(n, reps, native_scalar(v, n, t)), expect);
        emit(dist, n, qn[qi], "native-u32-sse2", 4, MEASURE(n, reps, native_sse2(v, n, t)), expect);
        if (v16 && t <= 0xFFFF)
            emit(dist, n, qn[qi], "native-u16-sse2", 2, MEASURE(n, reps, native_u16_sse2(v16, n, (u16)t)), expect);
        if (v8 && t <= 0xFF)
            emit(dist, n, qn[qi], "native-u8-sse2", 1, MEASURE(n, reps, native_u8_sse2(v8, n, (u8)t)), expect);
        emit(dist, n, qn[qi], "native-u32-zonemap", 4 + 8.0 / BLOCK,
             MEASURE(n, reps, native_zonemap(v, &z, n, t)), expect);
        double fb = (double)packed_bytes(&p) / n;
        emit(dist, n, qn[qi], "for-decode-all", fb, MEASURE(n, reps, packed_for_scan(&p, t, 0)), expect);
        emit(dist, n, qn[qi], "for-skip", fb, MEASURE(n, reps, packed_for_scan(&p, t, 1)), expect);
        if (delta) {
            double db = (double)packed_bytes(&pd) / n + 4.0 / BLOCK;
            emit(dist, n, qn[qi], "delta-decode-all", db, MEASURE(n, reps, packed_delta_scan(&pd, t, 0)), expect);
            emit(dist, n, qn[qi], "delta-skip", db, MEASURE(n, reps, packed_delta_scan(&pd, t, 1)), expect);
        }
    }

    if (with_ops) {
        char dist[64];
        snprintf(dist, sizeof dist, "%s-%d", dist_name, bits);
        u32 *back = xalloc(n * 4);
        emit_op(dist, n, "encode", "row",
                MEASURE(n, 3, (pack_column(&p, v, n, 0), (u64)p.bits[0])));
        emit_op(dist, n, "decode", "row", MEASURE(n, 10, (unpack_column(&p, back), (u64)back[n - 1])));
        emit_op(dist, n, "native-copy", "row", MEASURE(n, 10, (memcpy(back, v, n * 4), (u64)back[n - 1])));

        enum { GETS = 1 << 20 };
        size_t *idx = xalloc(GETS * sizeof(size_t));
        for (size_t i = 0; i < GETS; i++) idx[i] = ((u64)next_u32() << 16 ^ next_u32()) % n;
        u64 g;
        emit_op(dist, n, "random-read-native", "access",
                MEASURE(GETS, 3, ({ g = 0; for (size_t i = 0; i < GETS; i++) g += v[idx[i]]; g; })));
        emit_op(dist, n, "random-read-for", "access",
                MEASURE(GETS, 3, ({ g = 0; for (size_t i = 0; i < GETS; i++) g += packed_get(&p, idx[i]); g; })));

        // A write that fits the block's frame: rewrite each value with itself.
        emit_op(dist, n, "write-fits-for", "access",
                MEASURE(GETS, 3, ({ g = 0; for (size_t i = 0; i < GETS; i++) g += packed_set_in_place(&p, idx[i], v[idx[i]]); g; })));
        emit_op(dist, n, "write-native", "access",
                MEASURE(GETS, 3, ({ for (size_t i = 0; i < GETS; i++) back[idx[i]] = v[idx[i]]; (u64)back[0]; })));

        // A widening write: decode the block, change one value, re-encode
        // into a scratch buffer. Excludes moving later blocks, which a
        // contiguous variable-width layout would also need.
        __m128i *scratch = xalloc(32 * 16);
        u32 blk[BLOCK];
        emit_op(dist, n, "write-repack-block", "access",
                MEASURE(GETS / 16, 3, ({
                    for (size_t i = 0; i < GETS / 16; i++) {
                        size_t b = idx[i] / BLOCK;
                        unpack_block(&p, b, blk);
                        blk[idx[i] % BLOCK] = 0xFFFFFFF0u;
                        u32 mn = blk[0], mx = blk[0];
                        for (int k = 1; k < BLOCK; k++) { if (blk[k] < mn) mn = blk[k]; if (blk[k] > mx) mx = blk[k]; }
                        for (int k = 0; k < BLOCK; k++) blk[k] -= mn;
                        pack_block(blk, width_of(mx - mn), scratch);
                    }
                    (u64)((u32 *)scratch)[0];
                })));

        // Ordered removal at the front: native shifts n-1 values; packed
        // must decode and re-encode every later block. Reported per moved row.
        emit_op(dist, n, "remove-front-native", "row",
                MEASURE(n, 3, (memmove(back, back + 1, (n - 1) * 4), (u64)back[0])));
        emit_op(dist, n, "remove-front-for", "row",
                MEASURE(n, 3, (unpack_column(&p, back), memmove(back, back + 1, (n - 1) * 4),
                               pack_column(&p, back, n, 0), (u64)p.bits[0])));
        pack_column(&p, v, n, 0);
        free(idx);
        free(scratch);
        free(back);
    }

    free(v);
    free(v8);
    free(v16);
}

int main(int argc, char **argv) {
    perf_open();
    if (perf_fd < 0) fprintf(stderr, "warning: no perf counter; instructions read as 0\n");
    const size_t sizes[] = {16384, 1 << 20, 1 << 26};
    int only_size = argc > 1 ? atoi(argv[1]) : -1;
    for (int si = 0; si < 3; si++) {
        if (only_size >= 0 && only_size != si) continue;
        size_t n = sizes[si];
        int ops = si == 1;
        run_case("uniform", UNIFORM, 4, n, 0);
        run_case("uniform", UNIFORM, 8, n, ops);
        run_case("uniform", UNIFORM, 12, n, 0);
        run_case("uniform", UNIFORM, 16, n, 0);
        run_case("uniform", UNIFORM, 24, n, 0);
        run_case("outliers", OUTLIERS, 8, n, 0);
        run_case("sorted", SORTED, 4, n, ops);
    }
    return (int)(sink & 0);
}
