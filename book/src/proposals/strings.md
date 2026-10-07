# One immutable `str` with explicit lengths

Status: design proposal for this implementation batch. The implementation and
`DESIGN.md` remain the authority for currently supported behavior.

## Language contract

There is one public string type, `str`. It is an immutable sequence of Unicode
scalar values, stored as valid UTF-8. Assignment and passing arguments preserve
independent value semantics while sharing immutable storage internally. `mut`
permits replacing a binding; it does not make the string's bytes mutable. No
public owning-string/view-string distinction, implicit byte array coercion,
nullable string, or string-specific lifetime parameters are needed.

Preserve the existing surface: literals, concatenation, equality, `len`, integer
indexing, membership, iteration, typed arguments/results, and collection keys.
`len` counts Unicode scalar values, not bytes or grapheme clusters. Indexing
returns a one-scalar `str`, supports negative indices, and reports an out-of-range
index at runtime. A combining mark is its own scalar. Neither equality nor
hashing performs Unicode normalization. Surrogate code points are invalid.

Embedded U+0000 is ordinary content. Add the `\0` literal escape and allow a
literal NUL character inside a quoted string. Default storage has **no trailing
NUL byte**, and no operation searches for a terminator. This requirement applies
to static literals, empty strings, concatenation, indexing, and input alike.
Additional escape forms and methods can be added independently; this change
need not introduce slicing, formatting, case conversion, or a public `bytes`
type.

## Representation and ABI

Keep `Ty::Str` represented by one non-null machine-word pointer in Cranelift and
collection slots. The pointer addresses an immutable allocation containing a
common reference-counted object header, two unsigned 64-bit lengths, and inline
UTF-8 bytes:

```text
PlentyObject header       # {u64 refs, void (*destroy)(void *)}
u64 byte_len
u64 scalar_len
u8 data[byte_len]         # exact logical payload; no terminator
```

Use the ownership proposal's common header verbatim: `{u64 refs,
void (*destroy)(void *)}`. Offsets are `byte_len=16`, `scalar_len=24`, `data=32`.
The callback lets the base runtime release objects without linking against
optional collection or enum runtime code. Do not put a
second reference count in the string header. Runtime C owns layout details;
compiler literals must use the same documented offsets and 8-byte alignment.
Use a shared compiler-side layout definition and C static assertions to catch
drift. Current targets are 64-bit; reject unsupported targets instead of silently
serializing a mismatched header.

Heap strings start with one owned reference. Static literal headers use the
common immortal count sentinel (`UINT64_MAX`) and a null callback;
retain/release must inspect the sentinel
before attempting a write because literal data is read-only. Scalar count for
literals is computed once by the compiler. The literal's header and inline data
can occupy one read-only object symbol, requiring no data-pointer relocation.
The empty literal is a complete aligned header with both lengths zero.

The representation deliberately preserves `PTR_TY`, `clif_type(Ty::Str)`, source
function signature widths, local SSA variables, return values, and tail-call
signature compatibility. A two-register `(pointer, length)` source ABI would
force a much broader compiler rewrite without giving users an additional
capability here. No monomorphization or Unicode library is needed.

Check byte-count, scalar-count, and allocation-size addition before allocating.
Keep logical lengths at most `i64::MAX`, matching `len` and index types. Refcount
overflow must trap; it must never wrap into zero or the immortal sentinel.

## Runtime interface and ownership

Use ordinary C helper signatures taking `const PlentyStr *` and returning
`PlentyStr *` where appropriate. All observing arguments below are borrowed for
the duration of a call. Returned strings carry one owned reference. Generated
code releases its owned input temporaries after an observing helper returns;
helpers do not unexpectedly consume their inputs.

| Helper | Behavior |
| --- | --- |
| `plenty_concat(a, b) -> str` | Allocate exact combined byte length, copy bytes, add cached scalar counts |
| `plenty_str_eq(a, b) -> i8` | Compare byte lengths, then `memcmp`; pointer equality is a fast path |
| `plenty_contains(haystack, needle) -> i8` | Bounded byte-substring search; empty needle is always present |
| `plenty_println(s)` | `fwrite(data, 1, byte_len, stdout)` then newline |
| `plenty_print_str(s)` | Existing quoted legacy representation, bounded by length; escape NUL rather than dropping it |
| internal string length helper | Return cached scalar count |
| internal scalar index helper | Normalize signed index without overflowing; scan UTF-8 boundaries; return a fresh one-scalar string |
| internal string hash helper | Hash exactly `byte_len` bytes, including NUL, using existing hash algorithm |

Runtime helpers may return a retained existing string for empty concatenation
or other safe optimizations later; callers must already treat results as owned.
Strings have no child references, so releasing their last reference frees the
allocation directly. Collection insertion retains stored string keys/values;
projection retains the returned string; collection copying retains all copied
children; collection destruction releases them. Enum string payloads follow
the same contract. A string stored in a suspended generator is just another
owned frame field, and yielded strings obtain an independent owned reference.

String equality and hashing must be changed together. Otherwise embedded NUL
would corrupt dictionary/set key equivalence. Keep descriptor C strings as
compiler-internal NUL-terminated metadata: those are not Plenty string values.

`plenty_readline` is currently a legacy ABI returning a nullable pointer which
the compiler converts to `(str, bool)`. Keep that internal convention during
this migration: EOF may return null to the helper caller, but never creates a
null language string. Use `getline`'s explicit byte count, remove the optional
LF and preceding CR by adjusting that count, validate UTF-8, then construct the
length-bearing string and free the temporary C buffer. Preserve embedded NUL.
Invalid UTF-8 produces an explicit input diagnostic, rather than constructing
an invalid string or silently replacing bytes. Later modern input APIs should
use `Option`/`Result`; that API change is separate from storage migration.

Strict validation rejects overlong encodings, truncated sequences, stray
continuations, UTF-16 surrogates, and values above U+10FFFF. Compiler literals
are already valid Rust UTF-8; concatenation and scalar extraction preserve the
invariant. Validation is needed at external input boundaries only.

## Iteration cost and cursor interface

Existing `CollectionOp::IterGet(Ty::Str)` uses a scalar index and rescans from the
start. Simply replacing terminator checks with byte bounds fixes correctness
but leaves full iteration quadratic. Do not conceal this cost in the design.

For linear string iteration, use a compiler-private byte cursor. The loop keeps
an owned snapshot of the source, a byte offset, and the current one-scalar
string. Its condition compares byte offset with byte length; scalar extraction
copies the one-to-four bytes at that boundary. Its step adds the current
scalar's byte width. All three iteration consumers need this path: source
`for`, comprehension clauses, and `list`/`set` construction from iterables.
`continue` must execute the same cursor step before jumping to the condition.
The byte cursor is an implementation detail; ordinary `s[i]` remains indexed
by scalar number.

Suggested private operations are `TextByteLen(str) -> i64` and
`TextAtByte(str, i64) -> str`; they fit the existing fixed collection runtime
ABI. Existing scalar `Get(str)` remains separate. Every internal byte access
must enforce bounds and a UTF-8 boundary, even though well-formed generated
loops satisfy both. An alternative unified iterator object is appropriate if
the generator implementation already supplies one; it should own the string
snapshot and byte cursor, with exactly the same semantics.

For the smallest coherent first patch, retain scalar-index iteration, document
its cost, and land cursor iteration immediately afterward. This is a performance
staging choice, never a reason to retain NUL-terminated storage.

## Exact integration points

1. `runtime/plenty_runtime.c`: define the common-header string layout, bounded
   helpers, strict input validation, and allocation/destruction integration.
   Remove `strlen`, `strcmp`, `strstr`, and `fputs` from all language-string
   operations. `fputs` remains fine for literal diagnostics.
2. `runtime/collections.c`: replace string casts and scans in
   `collection_hash`, `collection_value_equal`, `collection_print_value`,
   `collection_text_length`, `collection_text_at`, and the descriptor-`s`
   dispatch. Delegate to shared string helpers, avoiding two inconsistent UTF-8
   implementations. The existing runtime concatenation build places the base
   runtime before collections, so shared helpers need no separate link step.
3. `src/codegen.rs`: change `declare_str_data` and `declare_eof_empty_str` to
   serialize complete aligned headers plus exact bytes in native target byte
   order. Update runtime comments and read-line fallback assumptions. Retain
   existing helper signatures at the CLIF level. String-pattern comparisons
   automatically use the new equality helper.
4. `src/frontend.rs`: decode `\0` and remove the in-quote literal NUL rejection.
   Do not loosen identifier or unquoted source token rules. Alias/type rules
   remain unchanged.
5. `src/frontend/collections.rs`, `src/collection.rs`, and
   `src/codegen/collections.rs`: add the optional byte-cursor lowering described
   above. Ordinary scalar indexing signatures remain unchanged.
6. The ARC integration pass described in `ownership.md` inserts retain/drop at
   local loads, overwrites, temporaries, calls, returns, and control-flow exits.
   Do not ship partial retain/drop insertion that can prematurely free strings
   still reachable through a collection, enum, frame, or legacy operand stack.
7. Update `DESIGN.md` to remove owning/view-type and C-string statements; teach
   one immutable `str`, scalar indexing, and embedded NUL in `TUTORIAL.md` with
   runnable examples. Update existing NUL-rejection diagnostic tests.

## Validation gates

- Static, empty, concatenated, returned, mutually tail-called, and nested
  collection strings behave identically across normal and legacy AOT paths.
- `len("a\0b") == 3`; indexing `1` returns `"\0"`; raw printing preserves
  the NUL byte; substring search can find text after or spanning it.
- `"a\0b" != "a\0c"`; dictionary/set keys distinguish those values and
  distinguish `"a"` from `"a\0"`.
- Empty-needle search, empty haystack, longer needle, final-byte matches, and
  equal-prefix unequal-length comparisons never read past payload bounds.
- One-, two-, three-, and four-byte scalars; combining characters; negative
  indexing; both index boundaries; empty iteration; nested comprehension
  iteration; `continue` and `break` all work.
- Input accepts valid UTF-8 including NUL and rejects each malformed encoding
  class, while preserving existing EOF and CRLF behavior.
- ARC tests cover strings surviving source-binding replacement, collection
  projection, enum matching, yield/resume, and early return; repeated string
  concatenation under AddressSanitizer/LeakSanitizer catches ownership leaks and
  use-after-free. No writes to immortal read-only headers.
- C runtime builds with warnings denied; formatting, Clippy, full tests, and
  executable tutorial examples pass. ASan/UBSan cover bounded operations using
  payloads deliberately lacking any accessible terminating byte.

## Future foreign interfaces

FFI must explicitly distinguish `(pointer, byte_length)` APIs from APIs requiring
terminated C strings. Exporting to the latter creates a temporary terminated
buffer and rejects embedded NUL, with lifetime tied to the call unless ownership
is explicitly transferred. Importing C text validates UTF-8 and copies into the
normal representation. DLL loading and FFI declarations are future work; they
must not dictate the default language string layout or add another ordinary
string type.
