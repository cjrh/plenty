# Borrowed text: evidence and naming

**Status: Evidence for the background [One borrowed text type](../unified-borrowed-text.md)
alternative, with the separate-view implementation recorded below.** The original
source assessment used `6bfe23c2` (2026-11-08) and the existing Plenty 0.8.17
debug binary. Its probes are historical evidence, not validation of subsequent
changes. Layout and throughput benchmarks have not been performed.

## Delivered separate-view design

Ordinary `&str` and generic `&T` references keep their existing meaning. The
separate two-word view is named `&strview`, with `&substr` as a compatible alias.
Scalar length, slicing, indexing, and iteration semantics remain unchanged;
view length scans and the view stores no scalar-count cache. View parameters
implicitly borrow whole strings; local view bindings still need an explicit
borrow or an existing reference. Returned/stored views remain separate B11 work.

Both forms expose allocation-free `as_bytes()` over the selected UTF-8 range.
Byte iteration uses that ordinary byte view; owned byte copying, extension,
search, membership, ASCII operations, splitting, replacement and joining use
[the binary API](../../design/stdlib/bytes.md#byte-views). Builtin text operations
and mixed string/view operands read ranges directly. Read-only standard-library
inputs use views where appropriate, with ownership boundaries and unchanged
protocol/getter exceptions recorded in the [input inventory](../../design/stdlib/text-input-views.md).
The generic-reference evidence below explains why this route preserves two
borrowed text forms. No generic-reference specialization or unification was implemented.

## Whole-handle retention boundaries

A two-word view carries a byte address and count, not the owning `str` handle.
Replacing every `&str` parameter with a view would force new payload copies
where existing code simply shares `*text`. Allocation-check regressions for
issue #135 exposed this distinction in `stdlib/std/http/client.plenty::request`,
`stdlib/std/http/cookie.plenty::set_cookie`, and
`stdlib/std/http/server.plenty::Router.add`. Their existing tests
`request_shares_its_method_and_url`, `set_cookie_shares_its_arguments`, and
`routes_and_captures_share_their_strings` preserve the original sharing budgets.

The implemented rule classifies each parameter: retain whole handles through
ordinary `&str`, inspect text through `&strview`, and explicitly copy a selected
view when an owner must escape. Mixed APIs keep both roles. The current
[inventory](../../design/stdlib/text-input-views.md#scope) includes compiled
regex patterns, template names, MIME stored values, and response Content-Type
handles. Focused allocation tests live in `tests/test_std_regex.rs`,
`tests/test_std_template.rs`, `tests/test_std_mime.rs`, and
`tests/test_std_http_server.rs`. Parsed tokens, normalized results and error
text retain their ordinary fallible construction contracts.

## Length, indexing and cached counts

Scalar semantics and scalar-count caching are independent decisions. UTF-8
stores variable-width scalars: knowing how many scalars a range contains does
not tell us where its scalar number 100 starts. A cached count supplies no
offset table. Sequential iteration should advance a byte cursor, rather than
repeatedly index from the beginning.

| Operation | Plenty today | Rust UTF-8 text | CPython `str` |
| --- | --- | --- | --- |
| Default length | Scalars; owned count cached or bounded inline scan; view scans | Bytes, constant-time | Code points, constant-time |
| Scalar/code-point position | Boundary traversal for non-ASCII text | No integer string indexing; `chars().nth(i)` traverses | Constant-time code-point indexing |
| Selected text range | Scalar bounds; borrowed view or fallible owned copy | Byte bounds; borrowed range requires UTF-8 boundaries | Ordinary selected substring copies its code points |
| Sequential text iteration | One-scalar `str` values | `char` scalar values via `chars()` | One-code-point `str` values |

Rust's [string explanation](https://doc.rust-lang.org/book/ch08-02-strings.html)
connects its rejection of integer string indexing to variable-width UTF-8 and
indexing cost expectations. Its [`str` API](https://doc.rust-lang.org/std/primitive.str.html)
provides byte length, byte slices, scalar iteration and scalar byte offsets;
it does not cache a scalar count. Computing `chars().count()` scans.
[`Iterator::nth`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.nth)
consumes preceding elements; it is not a scalar random-access index.
A Rust byte-range slice checks its boundaries without scanning all preceding
scalars. That differs from selecting a range by scalar positions.

CPython's [PEP 393 representation](https://peps.python.org/pep-0393/)
uses a uniform one-, two- or four-byte width within each string, chosen for its
largest code point, and stores its length. This permits direct code-point
addressing; the cached length alone is not the reason indexing is constant-time.
It trades UTF-8's compact mixed-width storage for that property. Python also
permits surrogate code points, so its elements are not precisely Plenty/Rust
Unicode scalars. Ordinary nontrivial slices copy selected code points; identity
and empty slices can be special cases. These are CPython representation facts,
not requirements on every Python implementation.

Plenty's owned header stores byte and scalar lengths in
`plenty-runtime/src/strings.rs` (`Text`, `scalar_len`, `is_ascii`, `at`, `try_slice`).
At most seven UTF-8 bytes fit inline; their scalar count is obtained by a bounded
scan. For a headed string, equal byte/scalar counts establish ASCII cheaply.
ASCII indexing then uses direct byte offsets. General Unicode indexing still
traverses boundaries. A view uses address plus byte count in
`plenty-runtime/src/aggregates/text_views.rs`; `TEXT_VIEW_LEN` scans, and current
indexing can count before traversing again. That extra scan is an implementation
optimization opportunity, not an inherent requirement of scalar semantics.

Useful cached-count workloads include repeated length queries on the same
range, length-based rejection before comparisons, and repeated ASCII checks
before scalar-position operations. Negative/clamped bounds can also benefit
from a known count. A single sequential traversal or byte-buffer copy does not
need the total scalar count. Repeated `len(view)` in a loop can create quadratic
work; hoisting it or caching explicitly avoids that without widening every view.

Copying **bytes** requires only the known byte count. Constructing an owned
Plenty `str` additionally needs its scalar-count invariant: a source view without
a count must count during construction or supply independently known metadata.
Returning an existing owned handle avoids that payload copy and recount.
No grapheme boundary information follows from a scalar count. Combining marks
and multi-scalar emoji still require segmentation for user-perceived characters.

## Byte access and binary operations

Implementation boundaries and original precedents:

| Source | Finding |
| --- | --- |
| `src/frontend/views.rs`, `view_argument` | `&strview` parameters already implicitly borrow whole `str` arguments, including call-scoped temporaries. Locals still need explicit borrowing. |
| `src/frontend/views.rs`, `text_view_iteration` | Iteration walks UTF-8 boundaries and produces one-scalar inline `str` values without computing total scalar length. |
| `src/frontend/views.rs`, `__regex_text_bytes` handling | Internal adapter forwards the text origin's loan to a shared byte-view argument. It is argument-only, not a general returned-view mechanism. |
| `src/regex.rs`, `RegexOp::TextBytes` | Records conversion from text view to byte view. |
| `src/codegen/collections.rs`, `lower_collection` | Regex conversion reuses the existing address/count operand with byte-view type; no byte allocation or copy. |
| `stdlib/std/regex.plenty`, `search_bytes` call using `__regex_text_bytes(text)` | Actual consumer of the loan-preserving byte adapter. |
| `stdlib/std/bytes.plenty`, `from_text` | Public owned-byte conversion accepts `&strview` and copies its `as_bytes()` range directly. |
| `stdlib/std/bytes.plenty`, `Bytes.extend`, `Bytes.overwrite` | Shared byte-view consumers that a public text-to-byte borrow should feed directly. |
| `stdlib/std/fmt.plenty`, `text`, `substr` | View-capable formatting entries share the range-capable output path; legacy helper names remain compatible. |
| `src/frontend/text_methods.rs`, `src/text.rs` | Range-capable builtin text dispatch and checked-view native adapter selection preserve ordinary owner fast paths. |
| `plenty-runtime/src/aggregates/text_ranges.rs` | Bounded range readers implement text operations without manufacturing an intermediate owning string. |

The original argument-only regex hook established the loan-preserving byte
adapter precedent. Public `as_bytes()` additionally propagates origins through
locals and nested calls, prevents mutation or movement of live inline storage,
and rejects escaping owners. This intrinsic result does not enable arbitrary
user functions to return stored or borrowed views.

The binary API accepts arbitrary bytes without UTF-8 validation. Lengths and
positions count bytes; ASCII transforms preserve non-ASCII bytes. Owned results
use fallible `std.bytes.Bytes` operations. Optional search indexes follow Plenty’s
convention. Python’s [bytes operations](https://docs.python.org/3/library/stdtypes.html#bytes-and-bytearray-operations)
provided the motivating comparison; the [byte reference](../../design/stdlib/bytes.md)
defines the delivered signatures and supported collection forms.

## Returned whole-string migration

An exact-signature search for `-> ... &str` across `stdlib/` found two public
getters: `std.mime.MediaType.type_name()` and `subtype()` in
`stdlib/std/mime.plenty`. Both borrow whole owned fields and remain ordinary `&str` getters in the
selected design. If unification were adopted, they could instead return `str`
by sharing the existing handle without allocating a text buffer. That optional
migration would require updating tests, reference signatures, explicit reference
annotations and dereferences; it is not required by the separate-view delivery.

This is a small **explicit standard-library getter** inventory, not a count of
all affected programs. Tests and the returned-reference reference also contain
returned `&str`, while generic returns can instantiate to the same type.
`tests/test_call_borrowing.rs`, `tests/test_returned_references.rs` and
`tests/test_optional_references.rs` cover direct and carried forms.
The [returned-reference rules](../../design/language/05-ownership/04-returned-references.md)
currently permit these signatures; they are not violations of a blanket ban.

Changing a return annotation alone works only if the body has access to an
owning handle. A helper receiving a range must instead take owned input or
make an explicit fallible copy. General returned text views need not be bundled
with whole-string getter migration.

## Generic-reference impact

Searches for generic `&T` parameters and `&T` results require body inspection:
protocol-bounded handles and SIMD values are not evidence of string breakage.
The following are concrete boundaries, not a measured downstream break count.

| Source | Consequence if shared `&T` specializes to a text view at `T = str` |
| --- | --- |
| `stdlib/std/cache/lru.plenty`, `get_ref`, `peek_ref` | Return `Option[&V]`. For string values this becomes a returned view, currently unsupported. Existing owned `get`/`peek` provide string alternatives, but globally returning `V` would break borrowing of noncopyable values. Needs an explicit generic API decision. |
| `stdlib/std/cache/lru.plenty`, shared `key: &K` paths | Forward keys into dictionary lookup. String-key hashing/equality must accept ranges without manufacturing an owner. |
| `stdlib/std/test.plenty`, `shown`, comparison helpers | Generic shared arguments reach comparison and representation intrinsics; audit text-view metadata and inference. |
| `stdlib/std/fmt.plenty`, generic display/debug helpers | Forward `&T` into formatting intrinsics. Adapt representation and ABI, even where source has no dereference. |
| `stdlib/std/serial.plenty`, `encode`; JSON/TOML generic entry points | Generic shared values reach serialization machinery; audit string specialization and generated hooks. |
| `tests/test_generics.rs`, `first[T]`, dereference fixtures | Element references and `*value -> T` exercise the changed ownership rule; string instances require migration/diagnostics. |
| `src/frontend/generics.rs` | Current view arguments are restricted at generic reference parameters. Type inference and specialization need deliberate changes. |

`str` satisfies today's `CopyType`; copying its owned handle is cheap. That
bound does not recover an owner from an arbitrary byte range. Likewise,
`&mut T` still refers to an owning slot, so its shared text reborrow becomes a
conversion when `T = str`. Tests must cover this alongside borrowed iteration,
matching and generic field/element projections. A new view name alone does not
remove these semantic decisions.

## Naming and type identity

| Candidate | Meaning and consequence |
| --- | --- |
| Unified `&str` | Whole and range borrows are views. Short spelling, but specializes shared `&T` for owned `str`. |
| Unified `&strview` | `&s` and `&s[a:b]` produce this view; whole-slot shared `&str` is removed or otherwise restricted. Makes the view visible in annotations, but still needs a rule for generic `&T` when `T = str`. |
| Rename only `&substr` to `&strview` | Keep current whole-slot `&str` separately. Improves the name for whole-string views and preserves ordinary generic references, but retains two borrowed text forms. |

`&strview` need not imply an owning, constructible `strview` type; that grammar
and type-identity choice must be explicit. All candidates can preserve current
implicit borrowing at view parameters and explicit local borrows. The rename-only candidate is implemented with a compatible legacy alias;
standalone owning `strview` is rejected. Unification remains undecided.

## Historical probes before implementation

The existing debug compiler accepted implicit whole-string arguments at
`&substr` parameters for owned variables, references and literals, and accepted
explicit borrowed locals. Assigning an owned string directly to a local
`&substr` binding was rejected. Iterating both owned text and a view over
`é`, `e`, a combining acute mark and an emoji produced scalar lengths of one
and byte lengths of two, one, two and four respectively.

It rejected a function returning `&substr` and rejected passing a view to
`std.bytes.from_text`; explicitly making an owned slice before that call was
accepted. The rejected `std.bytes.from_text` call identified a gap now fixed by the
separate-view delivery. These historical probes do not validate later changes
or generic unification; the view-returning function remains unsupported.

A further type-check probe accepted all of these in one program: a
`read[T: CopyType](value: &T) -> T` body using `*value` instantiated for `str`;
`first[T](values: &list[T]) -> &T` returning `&values[0]` for `list[str]`; and
`LruCache[str, str].get_ref("key")` alongside owned `peek("key")`. Thus the
generic examples are reachable current string instantiations, not merely
lexical matches. Reproduce using the single-file compiler's `--check FILE`
option with this core:

```text
from std.cache.lru import LruCache

def read[T: CopyType](value: &T) -> T:
    *value

def first[T](values: &list[T]) -> &T:
    &values[0]

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    text = "a longer owning string"
    print(read(&text))?
    values = [text]?
    print(first(&values))?
    mut cache = LruCache[str, str](2)?
    drop(cache.put("key", text)?)
    print(cache.get_ref("key"))?
    print(cache.peek("key"))?
    Ok(())
```
