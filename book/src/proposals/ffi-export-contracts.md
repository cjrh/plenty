# Generated library contracts and C headers

> **Status: Partly implemented.** Accepted 2026-10-07. Numeric and `Result`
> exports, owned and borrowed handles, generated headers, embedded interfaces,
> extraction, compatibility fingerprints, and typed resident loading are
> implemented. [Exports](../design/facilities/04-interop/exports.md),
> [owned exports](../design/facilities/04-interop/owned-exports.md), and
> [contracts](../design/facilities/04-interop/contracts.md) own the shipped
> contract. [C interop](ffi-and-dynamic-libraries.md) records the wider boundary
> design.
>
> **Shipped differently:** there is no standalone manifest. The contract is a
> generated `.plentyi` interface, embedded in the binary and written beside
> `NAME.h` and `NAME.link-args.txt`. The image API below is an illustrative
> sketch: generated headers differ in naming and in how `Result` exports report
> status. Exported error types include numeric values, `AllocError`,
> `ParseError`, `RangeError`, `Failure`, and nongeneric payload-free user enums;
> payload-bearing enums and error sums remain unsupported.
>
> **Remaining:** text and byte buffers, returned views, and broader error types
> (B27); callbacks, unloading, and loaders generated from extracted contracts
> (B28).

## One contract, several consumers

A Plenty-produced static or shared library should expose an ordinary C ABI and
carry a richer Plenty interface. Generate its C entry adapters, C header,
machine-readable `.plentyi` contract, and header contract comments from the same
checked export representation. Consumers using Plenty get ownership checking and
automatic cleanup; other languages get explicit instructions for satisfying the
same contract. They do not need a Plenty compiler to call the C exports.

The generated contract describes both the actual C signature and its Plenty
interpretation. For example, a status code and output handle may represent a `Result` containing
an owned object; its release symbol supplies automatic destruction. This does not
expose Plenty's private `Result`, class, or collection layouts as C layouts.
Borrow checking uses the interface without reanalyzing the library implementation.

Keep opaque handles and explicit C representations at this boundary. An owner
must be released through its originating library and allocator context. Ordinary
`pub` declarations do not become C exports or acquire a stable binary layout.
Generic exports need concrete specializations; metadata alone cannot instantiate
new machine code in an AOT consumer.

## Comments are part of the exported interface

Generate precise, human-readable comments beside each declaration. Use stable
templates driven by the contract, rather than inferred prose or a separately
maintained description of ownership. Author-written purpose, examples, and
domain-specific notes can supplement these generated facts, with their source
distinguished from the generated requirements and guarantees.

| Subject | What the header must explain when applicable |
| --- | --- |
| Inputs | Nullability, alignment, initialized extent, valid ranges, and required object/library identity. |
| Borrowing | Read/write access, alias restrictions, duration, whether pointers are retained, and whether callbacks can access the same storage. |
| Ownership | Whether an argument is borrowed or consumed; who owns every value on each success and failure path. |
| Outputs | Which outputs are initialized for each status, whether they must be initialized before the call, and whether previous contents are released. |
| Destruction | Exact release function and context, whether null is accepted, which aliases become invalid, and why ordinary `free` is inappropriate. |
| Returned views | The owner keeping a view alive and the mutations, calls, or destruction that invalidate it. |
| Text and buffers | Encoding, byte/element units, lengths and capacities, terminators, embedded NUL rules, and null behavior for empty buffers. |
| Failure | Status meanings, allocation failure behavior, partial effects, and whether input/output state is preserved. |
| Execution | Thread affinity, concurrent-call and reentrancy rules, callback lifetime, and prohibitions on unwinding or nonlocal jumps across the boundary. |
| Library lifetime | How long code, release functions, borrowed data, and callbacks must remain available. |

Describe obligations and guarantees separately. For example, an exclusive borrow
requires the caller to prevent conflicting access during the call; the callee
guarantees not to retain that borrow afterward. A C `const` qualifier alone does
not communicate these requirements. Avoid vague statements such as "caller owns
the result" when error paths or borrowed subobjects change what that means.

Illustrative generated header fragment, not an implemented image API:

```c
#include <stdint.h>

typedef struct ImagesImage ImagesImage;

#define IMAGES_OK UINT32_C(0)
#define IMAGES_INVALID_SIZE UINT32_C(1)
#define IMAGES_ALLOCATION_FAILED UINT32_C(2)

/**
 * Requires:
 * - image is a live, non-null handle from this library instance.
 * - Call on the thread that created image.
 * - No other access to image or its storage occurs during this call,
 *   including access through aliases or reentrant callbacks.
 * - No outstanding borrowed view into image's storage exists.
 * Guarantees:
 * - Borrows image exclusively for this call; retains no new alias.
 * - Ownership stays with the caller on every return path.
 * - IMAGES_OK: image has the requested dimensions.
 * - IMAGES_INVALID_SIZE: width or height is outside 1..65535 inclusive.
 * - IMAGES_ALLOCATION_FAILED: dimensions are valid but storage allocation fails.
 * - Both failure statuses leave image unchanged.
 * - These are the only returned status codes; runtime traps terminate
 *   the process and are not converted to status codes.
 */
uint32_t images_resize(ImagesImage *image, uint32_t width, uint32_t height);

/**
 * Requires:
 * - image is null, or a live owned handle from this library instance.
 * - For a live handle, call on its creating thread with no outstanding
 *   borrows and no concurrent or reentrant access to image or its storage.
 * - The originating library remains loaded throughout this call.
 * Guarantees:
 * - Null is a no-op; otherwise consumes and destroys image.
 * - All aliases and views into the destroyed object become invalid.
 * - Use this function exactly once per owned handle; never use free().
 */
void images_destroy(ImagesImage *image);
```

The unchanged-on-error promise above is a property of this illustrative API, not
a default for all exports. Emit only guarantees established by the checked
implementation or explicitly declared trusted dependencies. Do not invent
rollback, thread safety, non-retention, or error recovery from a C signature or a
function name. Unsupported contracts must produce export diagnostics rather than
silently generate a safe Plenty view. Explain trusted foreign assumptions where
they are part of the public contract.

Plenty checks its callers against these contracts. Foreign callers must uphold
the documented obligations themselves. Metadata and comments cannot stop invalid
C pointer use; runtime detection, where provided, is a separate API guarantee.

## Packaging and compatibility

Emit the versioned `.plentyi` interface beside the library and embed the same
contract in a dedicated binary section/resource or static-archive member; no
second manifest format is needed. Include target/ABI information,
stable exported type identities, contract format version, and an interface
fingerprint. Preserve discovery metadata through supported stripping, archive
extraction, and linker garbage collection. Read metadata at compile time without
loading or executing library code, and validate it as untrusted file input.

Runtime loading uses a library-specific versioned discovery symbol or function
table and checks the expected contract before exposing callable handles. Unique
symbol namespaces also permit multiple libraries in one static link. The
fingerprint detects interface mismatches; it is not authentication or proof of a
foreign implementation's behavior. AOT clients compile against known interfaces;
metadata does not provide arbitrary runtime specialization.

Keep loaded libraries resident initially. Any future unloading must account for
owners, release functions, borrowed data, callbacks, and in-flight calls. Library
output also needs a runtime packaging mode without application startup symbols;
metadata does not solve allocator or runtime-state composition by itself.

## Validation requirements

Compare generated header contracts with their `.plentyi` facts, including ownership
on failure and output initialization. Compile and run C callers against generated
headers for static and shared libraries, and test the corresponding Plenty
consumer's borrow and cleanup behavior. Cover unsupported contracts, incompatible
metadata, allocation failures, matching destruction, and the supported strip/link
settings. These checks belong with export and loader implementation, not solely
with documentation snapshots.

### Qualifying new boundary types

The [CO3 assessment](ffi-and-dynamic-libraries.md#co3-assessment-2026-10-22)
reinforces generating native representations and conversion code together. For
Plenty, extend this to a reviewable contract for **each supported conversion**,
rather than promising that every language type automatically has a public ABI.
Before admitting a new B27 buffer, view or error representation, specify:

- Its native declaration, target layout and valid values independently of the
  private Plenty layout; include pointer/length relations and tag/payload rules.
- Whether each direction borrows, copies or transfers, with allocator and library
  provenance, invalidation, and cleanup on every conversion failure.
- Which preconditions remain trusted and which the adapter checks; define the
  error result or termination behavior before converting invalid input into a
  language value. Validate readable storage only after its foreign validity
  obligations are satisfied; generated checks cannot safely probe arbitrary
  pointers.
- The semantic facts included in compatibility checking. A borrow-to-transfer
  change can break callers despite an identical C prototype. New lowering rules
  must update the contract/fingerprint scheme as necessary, with explicit
  treatment of old consumers; a stable-looking C layout alone is insufficient.

Qualify the same representative API through a C caller, a Plenty consumer of the
generated interface, and typed runtime loading where supported. Exercise static
and shared linking, valid success and error paths, allocation failure, and exact
destruction. For malformed-value tests, use valid backing storage with invalid
tags or encoding, and check size overflow before dereferencing buffers. A plausible
length exceeding the real allocation remains a foreign contract violation, not
something the adapter can generally detect. Use compiler rejection tests for
forbidden lifetime escapes. Test callbacks in both directions when
that capability is added. Compare results and ownership effects, not only emitted
header text or metadata. Existing C fixtures are the starting point, not a reason
to defer B27 until all B28 facilities exist.

CO3's finite specialization and tagged-dispatch examples are useful alternatives
for a later concrete generic-export need. Plenty should initially require named,
concrete wrappers. If specialization is added, record a finite exported set,
stable symbol names and full lowered contracts. Shared machine representation
does not imply interchangeable ownership or destructor semantics; tagged dispatch
also needs validated tags and an explicit unknown-tag policy. Neither generic
export syntax nor a universal companion-type framework is required for buffers,
views, header verification, or the first callbacks.
