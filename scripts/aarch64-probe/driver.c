#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <signal.h>
#include <stdlib.h>
#include <sys/wait.h>
#include <unistd.h>
#include <gnu/libc-version.h>
extern uint64_t probe_entry(void);
extern unsigned __int128 probe_u128(unsigned __int128, unsigned __int128);
extern uint64_t probe_trap(void);
unsigned __int128 c_add128(unsigned __int128 a, unsigned __int128 b) { return a + b; }
void c_touch(uint64_t *p) { p[0] = 23; p[16383] = 41; }
int main(void) {
    assert(probe_entry() == 250);
    unsigned __int128 a = ((unsigned __int128)0x123456789abcdef0ULL << 64) | UINT64_MAX;
    unsigned __int128 b = ((unsigned __int128)0xfedcba9876543210ULL << 64) | 2;
    assert(probe_u128(a, b) == a + b);
    pid_t child = fork();
    assert(child >= 0);
    if (child == 0) { probe_trap(); _exit(99); }
    int status;
    assert(waitpid(child, &status, 0) == child);
    assert(WIFSIGNALED(status) && WTERMSIG(status) == SIGILL);
    printf("ARM backend probe passed: glibc=%s page_size=%ld trap=SIGILL\n", gnu_get_libc_version(), sysconf(_SC_PAGESIZE));
}
