// Plenty AOT runtime — a tiny C library that compiled Plenty programs link
// against. Every helper has a fixed (non-variadic) signature so the
// Cranelift-emitted code can call it through a single declared signature,
// without depending on the host ABI's variadic conventions.
//
// The stack-print helpers reproduce the interpreter's `Vm::render` /
// `Vm::stack_repr` output exactly: integers carry their width suffix,
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

// String runtime — c.4. Strings are nul-terminated `const char *`s; the
// compiler emits one static-data symbol per source string literal, and
// runtime concatenation mallocs a fresh buffer. The heap is append-only
// (no free) to mirror the interpreter's `Heap` (DESIGN.md §12.1) — a
// real allocator and reclamation are a later concern.

// Print a string with the same escaping the interpreter's `Vm::render`
// uses (Rust's `{:?}` for `&str`): wrapped in double quotes, with `\`,
// `"`, and the common control chars (`\t`, `\n`, `\r`) backslash-escaped.
// Non-printable bytes outside that set are emitted as `\u{XX}` in
// lowercase hex — enough to match Rust for any ASCII string. Full
// Unicode-debug parity (escaping non-printable codepoints by name)
// would need a much larger table; AOT tests stay within ASCII for now.
void plenty_print_str(const char *s) {
    fputc('"', stdout);
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) {
        unsigned char c = *p;
        switch (c) {
            case '"':  fputs("\\\"", stdout); break;
            case '\\': fputs("\\\\", stdout); break;
            case '\t': fputs("\\t",  stdout); break;
            case '\n': fputs("\\n",  stdout); break;
            case '\r': fputs("\\r",  stdout); break;
            case '\0':                        break;  // unreachable: terminator
            default:
                if (c >= 0x20 && c <= 0x7e) {
                    fputc((int)c, stdout);
                } else {
                    fprintf(stdout, "\\u{%x}", (unsigned)c);
                }
                break;
        }
    }
    fputc('"', stdout);
}

// Concatenate two strings into a fresh malloc'd buffer with a trailing
// nul. The returned pointer is owned by the program and intentionally
// leaked — mirrors the interpreter's `Heap` which never reclaims.
const char *plenty_concat(const char *a, const char *b) {
    size_t la = strlen(a);
    size_t lb = strlen(b);
    char *out = (char *)malloc(la + lb + 1);
    if (!out) {
        fputs("plenty: out of memory in plenty_concat\n", stderr);
        abort();
    }
    memcpy(out, a, la);
    memcpy(out + la, b, lb);
    out[la + lb] = '\0';
    return out;
}

// Byte-for-byte string equality. Returned as `int8_t` (Plenty's Bool
// representation) so the caller can push it onto the value stack
// without further conversion.
int8_t plenty_str_eq(const char *a, const char *b) {
    return (int8_t)(strcmp(a, b) == 0);
}
