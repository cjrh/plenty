# Fuzzing

`plenty --fuzz FILE -o OUT` builds a [libFuzzer](https://llvm.org/docs/LibFuzzer.html)
executable from a program. libFuzzer runs the program's `main` once per
input, watches which code each input reaches, and keeps the inputs that reach
new code to mutate further. Inputs that trap are saved as findings.

```text
plenty --fuzz main.plenty -o fuzzer
./fuzzer -max_total_time=60 corpus/
./fuzzer crash-1060186d20321afe84b2d22eeca505a577c686c6   # replay one input
```

`--release` before `--fuzz` optimizes the generated code. `--libfuzzer PATH`
names libFuzzer's archive; the default is the one `clang
-print-file-name=libclang_rt.fuzzer_no_main.a` reports, which LLVM's
compiler-rt installs (on Fedora, the `compiler-rt` package). The build fails
with a message when there is none. `--linker` and `--link-arg` apply as for
`--compile`. The libFuzzer flags are LLVM's; `-help=1` lists them.

## Writing a target

The program reads the input through two C functions that the fuzz driver
defines. Declare them in a trusted module beside the program, for example
`fuzz_input.plentyi`:

```text
pub extern def size() -> u64 = "fuzz_input_size"
pub extern def byte(at: u64) -> u8 = "fuzz_input_byte"
```

`byte` returns 0 past the end. A target is an ordinary program that imports
this module, rebuilds the data it needs from the bytes, and calls the code
under test:

```text
import fuzz_input
import std.bytes

def main() -> Result[(), Failure]:
    mut data = std.bytes.empty()?
    mut at = 0u64
    while at < fuzz_input.size():
        data.push(fuzz_input.byte(at))?
        at += 1u64
    # parse `data` here
    Ok(())
```

A finding is a [fatal runtime error](entry-report.md) such as an index out
of range or `unwrap` of an `Err`, stack exhaustion, or `main` ending with a
nonzero status, including `Err`. libFuzzer reports the first as "fuzz target
exited" and the others as "deadly signal", saves the input, and exits with
status 77. To check a property, return `Err` when it fails.

Each input runs the whole program, as the runtime's `main` would, in the same
process, so state outside the program, such as files it writes, persists
between inputs. A target that serves a network protocol should feed the input
through an in-memory stream rather than a socket; `fuzz/http_server/main.plenty`
does this for [`std.http.server`](../../stdlib/http-qualification.md#fuzzing).

## Instrumentation

Every block of every function, including the standard library's, increments
its own byte counter when it runs. The counters and a table of their
functions are laid out as clang's `-fsanitize-coverage=inline-8bit-counters,pc-table`
lays them out, in sections `__sancov_cntrs` and `__sancov_pcs`; the driver
passes both to libFuzzer. `cov:` in libFuzzer's output counts the blocks
reached, and `ft:` the features, which also distinguish how often a block
ran.

Blocks are counted, not edges: a branch whose arms meet without running a
block of their own adds nothing. Integer comparisons are not traced. String
and byte-string equality reach `memcmp`, and libFuzzer's interceptors,
linked from `libclang_rt.fuzzer_interceptors.a` beside the `--libfuzzer`
archive when it exists, report the strings compared, which libFuzzer then
tries in inputs. A dictionary (`-dict=FILE`) of the format's keywords helps
with the rest.

Instrumented functions are cached like any other; their entries are separate
from those of ordinary builds. Fuzz builds keep the counters and table of
every function, so `--gc-sections` drops nothing they compile. Ordinary
builds contain neither the counters nor the driver.

## The driver

`plenty-runtime/native/fuzz_driver.c` is compiled with the compiler and
linked only into fuzz builds, with `-Wl,--wrap=main`: the C library starts
the driver, which registers the counters and calls libFuzzer's
`LLVMFuzzerRunDriver`, and each input calls the runtime's `main`. libFuzzer
is C++, so the link adds `-lstdc++`.

Parallel runs use `-jobs=N -workers=N`, independent processes sharing the
corpus directory. With the interceptors linked, libFuzzer's `-fork` mode
crashes its own parent process within seconds (clang 22.1.8), reporting a
"deadly signal" with an empty input; it is not a finding.

libFuzzer's handlers for `SIGSEGV` and `SIGBUS`, which save the input, run
on the faulting thread's stack. So that they still run when a program has
exhausted its stack, the driver gives the thread running inputs a 256 KiB
alternate stack and moves the handlers onto it. The runtime's own
stack-overflow message is not printed in fuzz builds.

Without AddressSanitizer libFuzzer has no leak checking or stack traces.
Plenty code needs neither for memory safety; a trap prints its source
position before libFuzzer's report.

The target is x86-64 Linux, as for every build (see [linking](linking.md)).
