# memmap: independent package experiment

This is an **unpublished, Linux-only experiment** for issue #480, staged here
so it can be reviewed and extracted into its own repository. The intended home
is a separate package under the plentylang organization, alongside `tz` and
`unicode_text`. It adds no `std.memmap`, compiler hooks, or runtime entry points.
No repository URL or released version is available yet. The manifest version
`0.0.1` identifies this local prototype, not a published release.

It creates a real, demand-paged, read-only `MAP_SHARED` mapping without eagerly
copying the file. It owns and unmaps the mapping on scope exit. Reads copy at most
the caller's existing buffer size; **there is no zero-copy Plenty byte view**.
Current foreign adapters cannot return a view into native storage. The original
issue's byte-source interoperability is therefore not delivered by this prototype.

## Build and try

Requires GNU/Linux, a C11 compiler, `make`, and Plenty 0.8.17 or newer. The tested
host is Linux x86-64; ARM64 and other Linux environments are not qualified.

```sh
cd experiments/packages/memmap
make
make check PLENTY=/absolute/path/to/plenty
```

The native shim is `target/libmemmap.so`. Package fetching does not build it;
there is no native build hook. Applications pass its path explicitly to `load`.
The shared library needs only libc and knows no Plenty object layouts. Copy this
whole package directory elsewhere to experiment without changing Plenty itself.
Both license files and the native tests belong to the standalone package.

A consumer can use a local dependency:

```toml
[dependencies]
memmap = { path = "../memmap", version = "0.0.1" }
```

Its `src/main.plenty` can contain:

```plenty
import memmap

def main() -> Result[(), AllocError | IoError | LoadError | memmap.MapError]:
    library = memmap.load("../memmap/target/libmemmap.so")?
    mapping = library.open("data.bin", max_bytes=1048576)?
    mut buffer = [0u8, 0u8, 0u8, 0u8]?
    count = mapping.read_into(0, &mut buffer)?
    print(mapping.length(), count, buffer)?
    Ok(())
```

The library path is relative to the application's working directory. As with
other native loading, only load code you trust. `load` checks the exact versioned
experiment contract and all required symbols before returning a usable table;
that is ABI checking, not authentication. Successfully loaded code stays resident
until process exit, so a mapping can safely outlive its `Library` value.

## API and ownership

| API | Contract |
| --- | --- |
| `load(path: &str) -> Result[Library, LoadError]` | Load and check the native backend using existing Plenty loader primitives |
| `Library.open(path: &str, max_bytes: NonNegative[i64]) -> Result[Mapping, MapError]` | Open a regular file, check its initial size against the explicit limit, and map it read-only; empty files are valid |
| `Mapping.length() -> u64` | Initial mapped length, unchanged by later writes, growth or truncation |
| `Mapping.read_into(offset: NonNegative[i64], output: &mut list[u8]) -> Result[i64, MapError]` | Copy up to the lesser of buffer length and initially mapped remainder, returning bytes written; never resize or allocate a Plenty buffer |

`Mapping` has private native pointers, a private constructor, and a destructor.
It cannot be copied or used on workers; neither the opaque types nor the indirect
foreign calls claim worker eligibility. `Library` also stays on its creating
thread. There is no writable mapping, flush, returned pointer, stored byte view,
streaming cursor, implicit conversion or unsafe opt-out. Slices of the destination
list are not yet accepted by the foreign mutable adapter; pass a reusable list.
Native opening allocates one small owner and OS mapping metadata. Closing the
original file descriptor after mapping does not release the mapping.

`MapError.InvalidOffset` means an offset beyond the original length.
`MapError.Path(CStrError)` preserves NUL/conversion allocation errors.
`MapError.OperatingSystem(i32)` contains Linux errno captured inside the shim
before cleanup; examples include `ENOMEM` for native allocation refusal,
`EFBIG` for the size limit, and `EINVAL` for a nonregular file. Loader failures
use the existing typed prelude `LoadError`. Failed acquisition returns no owner
and cleans up its descriptor/storage. Destruction makes a best-effort `munmap`
and cannot report an error.

## External changes and truncation

A read-only mapping does **not** make the backing file immutable. `fstat` cannot
prevent another opener truncating it immediately afterwards. A plain userspace
load or `memcpy` can deliver SIGBUS, so this backend never dereferences mapped
file bytes in userspace. Instead it calls Linux `process_vm_readv` on its own
address space, letting the kernel fault in and copy the requested pages. An
inaccessible/truncated page yields `EFAULT` or a short positive read. No signal
handlers, global signal policies, or `setjmp` recovery are installed.

A positive short read has written only the returned prefix; callers must use
that count. Retrying at the next offset can fail. Empty reads and an offset
exactly at the initial end return zero. The mapping does not include later file
growth. A partially truncated last page may still yield the kernel's zero-filled
page tail within the original mapped extent. These are mapping semantics, not a
current-file-length read contract. Writes can produce mixed-time data; a read is
not an atomic snapshot and does not guarantee a coherent structured record.

The syscall can be blocked by seccomp or another OS policy even for the current
process; the resulting errno is returned, with no fallback to unsafe memory
loads. The development sandbox does block it; native tests must run where the
syscall is permitted. This restriction is part of the experiment's portability
cost, not a reason to change application signal handlers or permissions.

## What this experiment establishes

The current FFI can support a separate owning mapping package, recoverable OS
errors and bounded caller-buffer reads without changing the compiler. It cannot
express the original issue's zero-copy borrowed byte source. Those two facts are
kept separate: the implementation uses actual mmap, but its public read still
copies through a syscall.

For ordinary buffered reading, prefer the existing file APIs. `make bench`
compares this mapped copy-out with `pread` using the same reusable 64 KiB buffer,
a warmed 64 MiB sparse file, 1 GiB per pass, alternating order over four rounds.
On the development Linux x86-64 host, mapped copy-out measured 5.39–5.55 GiB/s
versus 5.81–6.18 GiB/s for `pread`; it was slower. This small cache-hot probe is
not a representative workload or a performance promise. The prototype currently
provides an experimental mapping lifetime/ABI boundary rather than a demonstrated
application advantage over file reads. It should not become a recommended file
reader or standard-library API on that evidence.

The design question exposed by the experiment is how Plenty should admit foreign
byte sources while preserving its ownership and fault guarantees. Immutable sealed
backing storage and file-backed storage with external mutation are different
contracts; an ordinary shared borrow alone cannot assert either. Keeping the
package independent allows those choices to be explored without promising a
stable standard-library API. Remaining issue scope and priorities stay in
`book/src/backlog.md` in the Plenty repository, not in this package README.

## Validation

`make check` runs native boundary/truncation tests and all five Plenty package
tests in both optimized and fast-compile modes. Native coverage includes empty
and nonregular files, mapping limits, strided destination padding, EOF/offsets,
partial-page faults, already-faulted pages after truncation, and 2,000 reads
racing with 2,000 truncate/grow pairs in another process. Plenty tests also cover
loader/contract/path errors and a mapping outliving its library table.

For native allocation/lifetime instrumentation:

```sh
clang -std=c11 -Wall -Wextra -Werror -O1 -g -fsanitize=address,undefined \
  native/memmap.c native/test_memmap.c -o target/test-sanitized
./target/test-sanitized
```

The prototype has no Rust changes and wraps no third-party library or data.

## Acknowledgements

- [Linux man-pages: process_vm_readv](https://man7.org/linux/man-pages/man2/process_vm_readv.2.html)
  documents partial transfers, inaccessible-memory errors and the lack of atomic
  transfer guarantees; it informed the bounded copy-out contract. No manual text
  or sample implementation is shipped.
