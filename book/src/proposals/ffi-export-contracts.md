# Generated library contracts and C headers

Status: accepted design direction, 2026-10-07; numeric exports, generated headers,
and embedded `.plentyi` metadata are implemented in the
[initial subset](../design/24-c-interfaces/02-library-exports.md). Richer ownership
and compatibility examples below remain design sketches. Tasks and priorities live only in the
[backlog](../backlog.md). See [C interfaces](../design/24-c-interfaces.md) for
implemented imports and [C interop](ffi-and-dynamic-libraries.md) for the wider
boundary design.

## One contract, several consumers

A Plenty-produced static or shared library should expose an ordinary C ABI and
carry a richer Plenty interface. Generate its C entry adapters, C header,
machine-readable manifest, and header contract comments from the same checked
export representation. Consumers using Plenty get ownership checking and
automatic cleanup; other languages get explicit instructions for satisfying the
same contract. They do not need a Plenty compiler to call the C exports.

The manifest describes both the actual C signature and its Plenty interpretation.
For example, a status code and output handle may represent a `Result` containing
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

Emit a versioned standalone manifest and embed the same contract in a dedicated
binary section/resource or static-archive member. Include target/ABI information,
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

Compare generated header contracts with their manifest facts, including ownership
on failure and output initialization. Compile and run C callers against generated
headers for static and shared libraries, and test the corresponding Plenty
consumer's borrow and cleanup behavior. Cover unsupported contracts, incompatible
metadata, allocation failures, matching destruction, and the supported strip/link
settings. These checks belong with export and loader implementation, not solely
with documentation snapshots.
