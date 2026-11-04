/* C counterparts of kernels.plenty, written as fannkuch.c writes them. */
typedef long long i64;

void reverse(i64 *s, i64 first) {
    i64 lo = 1, hi = first - 1;
    while (lo < hi) {
        i64 t = s[lo];
        s[lo] = s[hi];
        s[hi] = t;
        lo++;
        hi--;
    }
}

void rotate_left(i64 *p, i64 last) {
    i64 h = p[0];
    for (i64 i = 0; i < last; i++)
        p[i] = p[i + 1];
    p[last] = h;
}

void copy_row(const i64 *perm, i64 *s, i64 n) {
    for (i64 i = 0; i < n; i++)
        s[i] = perm[i];
}

i64 count_flips(i64 *s) {
    i64 first = s[0], f = 0;
    while (first) {
        i64 lo = 1, hi = first - 1;
        while (lo < hi) {
            i64 t = s[lo];
            s[lo] = s[hi];
            s[hi] = t;
            lo++;
            hi--;
        }
        i64 nx = s[first];
        s[first] = first;
        first = nx;
        f++;
    }
    return f;
}
