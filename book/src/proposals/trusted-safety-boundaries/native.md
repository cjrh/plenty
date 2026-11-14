# Foreign ownership and explicit trust

Companion to [Stronger safety boundaries](../trusted-safety-boundaries.md).
Status: **Proposed; not implemented.** Current declarations and obligations are
in [trusted declarations](../../design/facilities/04-interop/trusted-declarations.md)
and [raw host interfaces](../../design/facilities/04-interop/raw-host-interfaces.md).
The descriptions below specify intended contracts, not final syntax.

## Trusted modules and unchecked exports

Require source that declares or directly uses unchecked native operations to
belong to an explicitly trusted module. Classification includes raw loads and
stores, pointer arithmetic, indirect native calls, native lifetime promises,
and foreign release operations. `.plentyi` already identifies declarations;
the new boundary must also include the Plenty implementation that uses them.

A trusted module can publish checked wrappers. Such a wrapper must establish
every safety precondition its callers cannot prove: bounds, initialization,
ownership, lifetime, synchronization, and any native protocol state. A public
function is not safe merely because its implementation lives in a trusted
module; the wrapper's promise remains manually reviewed.

An operation whose preconditions remain the caller's responsibility must be
an explicit unchecked export. Importing or calling it requires trusted source.
This obligation must follow re-exports, aliases, generic instantiations, and
stored function values. Wrapping an unchecked call in another function cannot
erase the obligation unless a trusted implementation explicitly provides the
checked boundary. Diagnostics should show the operation and dependency path.

For example, a checked byte-buffer wrapper knows its allocation and length,
checks an index, and returns a scalar. A public raw load accepting an opaque
pointer and offset has no such proof; it delegates obligations to trusted
callers even when its ABI declaration is exact.

Exports to native callers retain host obligations. Generated headers and import
contracts must identify ownership, lifetime, concurrency, callback context, and
unchecked delegation consistently. Trust facts must survive separate compilation
and participate in interface fingerprints and compiled-cache identities.

## Checked foreign owners and borrows

Introduce a compiler-recognized distinction between a foreign owner, a borrowed
foreign handle, and a raw pointer. Unlike today's conventionally owning class,
a checked foreign owner must not be constructible by copying an opaque address
in ordinary source.

| Value | Intended contract |
| --- | --- |
| Owner | Affine; acquired through a trusted constructor; releases its resource at most once |
| Shared borrow | Cannot outlive its owner; permits only declared shared native operations |
| Exclusive borrow | Prevents conflicting use or release for the loan's duration |
| Raw pointer | Address with manually upheld validity; construction, extraction, and access remain trusted |

Release consumes the owner; ordinary use afterward is rejected. Ownership
transfer into C also consumes it. APIs that consume only on success must return
the intact owner on failure, matching existing [transfer policies](../../design/facilities/04-interop/foreign-owners.md#transfer-policies).
Acquisition failure, nullability, and partially initialized resources need
explicit representation rather than a destructor reading invalid state.

The checked type must preserve provenance through aggregates and generic calls.
It cannot expose an ordinary constructor accepting a copyable raw pointer.
Borrowed views of native storage require a known extent and an owner-bound
lifetime, and may be admitted only when external mutation or revocation cannot
invalidate them. Memory mapping a file that another process can truncate does
not gain safety from an owner borrow alone.

Native libraries may use reference counting, immortal handles, or external
ownership. These need distinct trusted constructors and clone/borrow policies;
the design must not pretend every native address has a unique owner. Native
retention of a loan stays forbidden unless a separate checked retention
contract is introduced.

Reuse existing mobility and sharing checks where possible. A native library's
thread affinity or shared mutation remains a trusted fact; an owner type does
not establish it. Host-supplied pointers and arbitrary native code also remain
outside the compiler's proof.

## Application control of transitive trust

Provide a deterministic build report listing application/dependency trusted
modules, unchecked exports, native libraries, and their dependency paths. Show
the built-in compiler/runtime foundation separately; a source-only application
still relies on it.

Allow an application to reject additional trusted source or admit a specific
set of bindings. Apply the policy transitively to the resolved program, with
package identity, version/source identity, and relevant native artifacts rather
than an unqualified module name. Renaming or re-exporting a dependency must not
bypass admission. Generated bindings, loaded-library capabilities, and linker
inputs need an explicit classification too.

The application controls admission; a dependency cannot grant itself permission.
Reports identify *where* trust enters, not whether a library is trustworthy.
Changing a policy must invalidate affected cached checks. Standalone files,
scripts, packages, and library builds need the same rules and a documented
default; proposed manifest names remain open.

This policy is not a sandbox against a malicious compiler, arbitrary build
commands, or native code already running in the process. Build-time execution
permissions are a separate concern. It makes source and link-time trust
expansion explicit and reviewable.
