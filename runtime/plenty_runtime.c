// Plenty AOT runtime — a tiny C library that compiled Plenty programs link
// against. Every helper has a fixed (non-variadic) signature so the
// Cranelift-emitted code can call it through a single declared signature,
// without depending on the host ABI's variadic conventions.
//
// Legacy stack-print helpers render integers with their width suffix;
// values are space-separated, and the whole stack is bracket-wrapped with
// a trailing newline. The compiled program emits a sequence of these
// calls for each `.` (Display) word.
//
// `plenty_main` is the symbol Cranelift produces for the top-level
// program; the real `main` lives here and just forwards to it. That
// keeps the Cranelift entry point typed as `() -> i32` and avoids
// distinguishing the platform's `main(argc, argv)` shape from the IR
// builder's tidy zero-arg signature.

#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

// Modern Plenty's print(value): no quotes or integer-width suffixes.
void plenty_println_signed(int64_t n) { printf("%lld\n", (long long)n); }
void plenty_println_unsigned(uint64_t n) { printf("%llu\n", (unsigned long long)n); }
void plenty_println_bool(int8_t b) { puts(b ? "True" : "False"); }

extern int32_t plenty_main(void);

int main(int argc, char **argv) {
    (void)argc;
    (void)argv;
    return (int)plenty_main();
}

void plenty_print_i8 (int8_t  n) { printf("%di8",  (int)n); }
void plenty_print_i16(int16_t n) { printf("%di16", (int)n); }
void plenty_print_i32(int32_t n) { printf("%di32", n); }
void plenty_print_i64(int64_t n) { printf("%lldi64", (long long)n); }
void plenty_print_u8 (uint8_t  n) { printf("%uu8",  (unsigned)n); }
void plenty_print_u16(uint16_t n) { printf("%uu16", (unsigned)n); }
void plenty_print_u32(uint32_t n) { printf("%uu32", n); }
void plenty_print_u64(uint64_t n) { printf("%lluu64", (unsigned long long)n); }
void plenty_print_bool(int8_t b) { fputs(b ? "true" : "false", stdout); }

void plenty_print_open_bracket(void)  { fputc('[',  stdout); }
void plenty_print_close_bracket(void) { fputs("]\n", stdout); }
void plenty_print_space(void)         { fputc(' ',  stdout); }

// Trap helpers — c.5.5. Compiled programs branch here when an
// arithmetic operation overflows or divides by zero. Both helpers print
// a diagnostic (`error: <msg>`) and exit with status 1. They never return; the
// caller in generated code follows the call with a CLIF `trap` to
// satisfy the verifier's block-terminator requirement.
_Noreturn void plenty_trap_overflow(void) {
    fputs("error: integer overflow\n", stderr);
    exit(1);
}

_Noreturn void plenty_trap_div_zero(void) {
    fputs("error: division by zero\n", stderr);
    exit(1);
}
