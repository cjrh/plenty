#define _GNU_SOURCE
#pragma STDC FENV_ACCESS ON
#include <assert.h>
#include <dlfcn.h>
#include <fenv.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    assert(argc == 2);
    void *library = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!library) { fprintf(stderr, "%s\n", dlerror()); return 1; }
    double (*scale)(double,double,double,double);
    double (*dot)(double,double,double);
    void *s = dlsym(library, "vec_scale"), *d = dlsym(library, "vec_dot");
    assert(s && d); memcpy(&scale, &s, sizeof scale); memcpy(&dot, &d, sizeof dot);
    const int modes[] = {FE_TONEAREST, FE_DOWNWARD, FE_UPWARD, FE_TOWARDZERO};
    for (unsigned i=0; i<sizeof modes/sizeof modes[0]; ++i) {
        assert(fesetround(modes[i]) == 0);
        feclearexcept(FE_ALL_EXCEPT);
        double answer = scale(1.0, 2.0, 3.0, INFINITY);
        assert(isinf(answer) && answer > 0 && !(fetestexcept(FE_INVALID)));
        feclearexcept(FE_ALL_EXCEPT);
        answer = scale(1.0, 2.0, 3.0, -INFINITY);
        assert(isinf(answer) && answer < 0 && !(fetestexcept(FE_INVALID)));
        feclearexcept(FE_ALL_EXCEPT);
        answer = dot(1e16, -1e16, 2.0);
        assert(answer == 2.0 && !(fetestexcept(FE_INVALID)));
    }
    assert(fesetround(FE_TONEAREST) == 0);
    puts("12 source-compiled IEEE/fenv cases passed");
    assert(dlclose(library) == 0);
}
