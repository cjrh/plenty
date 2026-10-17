/* The libFuzzer entry of `plenty --fuzz` executables; see
 * book/src/design/toolchain/01-building/fuzzing.md. The compiler links this
 * object only into fuzz builds, with `-Wl,--wrap=main`, so the C library
 * starts here instead of at the runtime's `main`, which each input then runs
 * as a whole program. Shipped programs never contain it.
 *
 * The program reads the input through `fuzz_input_size` and
 * `fuzz_input_byte`, declared in a trusted `.plentyi` module. */
#define _GNU_SOURCE
#include <signal.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

int __real_main(int argc, char **argv);
int LLVMFuzzerRunDriver(int *argc, char ***argv,
                        int (*one)(const uint8_t *data, size_t size));
void __sanitizer_cov_8bit_counters_init(uint8_t *start, uint8_t *stop);
void __sanitizer_cov_pcs_init(const uintptr_t *start, const uintptr_t *stop);

/* The linker gathers every function's counters and PC entries here, in the
 * same function order. */
extern uint8_t __start___sancov_cntrs[], __stop___sancov_cntrs[];
extern const uintptr_t __start___sancov_pcs[], __stop___sancov_pcs[];

static const uint8_t *input;
static size_t input_size;
static char *arguments[2];

uint64_t fuzz_input_size(void) { return input_size; }

/* Zero past the end: a harness that reads out of range finds no input, not
 * someone else's memory. */
uint8_t fuzz_input_byte(uint64_t at) { return at < input_size ? input[at] : 0; }

#define SIGNAL_STACK (256 * 1024)

/* libFuzzer's SIGSEGV and SIGBUS handlers, which save the input, run on the
 * stack that faulted. When the program exhausted that stack the kernel then
 * kills the process before they run, and the input is lost. So the thread
 * running inputs gets an alternate stack and the handlers move to it. The
 * runtime's own stack-overflow handler stands aside for libFuzzer's. */
static void handle_on_alternate_stack(void) {
    static __thread int prepared;
    if (prepared) return;
    prepared = 1;
    stack_t stack = {.ss_sp = malloc(SIGNAL_STACK), .ss_size = SIGNAL_STACK, .ss_flags = 0};
    if (stack.ss_sp == NULL || sigaltstack(&stack, NULL) != 0) return;
    int signals[] = {SIGSEGV, SIGBUS};
    for (size_t i = 0; i < sizeof signals / sizeof signals[0]; i++) {
        struct sigaction action;
        if (sigaction(signals[i], NULL, &action) == 0 && (action.sa_flags & SA_SIGINFO)) {
            action.sa_flags |= SA_ONSTACK;
            sigaction(signals[i], &action, NULL);
        }
    }
}

/* A trap ends the process inside the run, which libFuzzer reports as the
 * target exiting, with the input saved. A nonzero status, such as `main`
 * returning `Err`, is reported the same way. */
static int run_one(const uint8_t *data, size_t size) {
    handle_on_alternate_stack();
    input = data;
    input_size = size;
    int status = __real_main(1, arguments);
    input = NULL;
    input_size = 0;
    if (status != 0) {
        fprintf(stderr, "error: the fuzz target's main returned status %d\n", status);
        fflush(stderr);
        abort();
    }
    return 0;
}

int __wrap_main(int argc, char **argv) {
    arguments[0] = argv[0];
    __sanitizer_cov_8bit_counters_init(__start___sancov_cntrs, __stop___sancov_cntrs);
    __sanitizer_cov_pcs_init(__start___sancov_pcs, __stop___sancov_pcs);
    return LLVMFuzzerRunDriver(&argc, &argv, run_one);
}
