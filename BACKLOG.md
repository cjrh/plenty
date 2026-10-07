# Backlog

Implementation work for the AOT-only compiler. These are candidate approaches,
not committed milestones. The language contract and roadmap are in
[reference](book/src/design/index.md).

---

## Portability

### Pointer width is hard-coded to 64 bits

`clif_type(Ty::Str)` returns the module-private constant `PTR_TY =
types::I64`. Correct on every host we actually target today, but a
32-bit cross-compile would silently miscompile every string value.

The fix is one line: read `module.target_config().pointer_type()`
inside `clif_type` for the `Str` case. Plumbing it requires threading
the `ObjectModule` (or its target config) into `clif_type`'s callers
— a mechanical change, kept undone only because no test would
currently exercise it.

---

## Performance / size optimisations

None of these are demanded by current users. Each is a local change.

### Heap reclamation

`plenty_concat` in `runtime/plenty_runtime.c` `malloc`s a fresh
buffer per call and never `free`s. A long-running native binary that builds
many strings retains them until exit. Resolve string ownership and view types
alongside the move/drop design, then implement reclamation in the native runtime.

### Fat-pointer strings

Strings are nul-terminated `*const u8`. Concat re-scans for length
via `strlen`. A `(ptr, len)` fat pointer would skip the scan and
make string ops O(1) in the small-string case. The downsides: a
`Ty::Str` value no longer fits a single CLIF SSA `Value` (needs two,
or a packed i128); pattern-match scrutinees need different
plumbing.

Local to the runtime + the `Ty::Str` row in `clif_type`.

### Switch-table lowering for dense integer matches

`lower_match` emits a linear `brif` chain regardless of arm count.
Fine for the kinds of matches a learner writes (2–4 arms); a
hundred-arm match on `:i32` would benefit from a jump table.
Cranelift has `br_table` — the work is recognising when the
integer arms are dense enough to be worth the table and emitting
the table block.

### Precompiled-runtime archive

`compile_source_to_executable` writes the embedded runtime C source
to a tempfile per invocation and lets `cc` recompile it. Both `plenty FILE`
and `plenty --compile` pay this cost. Measure runtime compilation and linking
separately before optimizing; the baseline in the reference measures the complete
AOT pipeline, not runtime compilation alone. A candidate approach is:
a `build.rs` that compiles `plenty_runtime.c` into
`libplenty_runtime.a` at crate build time, an `include_bytes!` of
the archive bytes into the binary, write the archive at link time
instead of the source. Localized to `codegen.rs` + the new
`build.rs`; no API change.

---

## CLI / packaging

### Linker override

`compile_source_to_executable` hard-codes `cc` from `PATH`. No
`$CC` env var, no `--linker` flag. If a user with no `cc` (e.g.
only `clang.exe` or `cl.exe` on Windows) shows up, the natural
addition is a `--linker /path/to/cc` flag — applied after the
default lookup, no behaviour change for existing users.

### Windows support

Untested. The compile path uses Cranelift (which emits COFF on
Windows targets) plus `cc` (which most Windows users don't have by
default). At minimum the link step would need to default to
`cl.exe` or `clang.exe` with different argument syntax, and the
temp-file extensions (`.o` vs `.obj`, `.exe` mandatory for the
output) would need attention.

### `--emit obj`

`--compile` always produces an executable; the object file is a
private implementation detail. If a real use case appears for the
raw `.o` (linking with non-trivial C code, packaging into a static
library, etc.) the strict-form `--emit obj` flag would expose it
without bringing back the suffix-sniffing the original c.5 sketch
considered.
