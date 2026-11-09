# Exports called by host threads

`@thread_safe` on an `export def` checks the reachable Plenty body with the
[native worker effect rules](../03-concurrency/effects.md). The annotation
permits host threads to call that entry concurrently on independent storage,
and permits sequential handoff of an instance between threads. The host must
synchronize handoff and keep all pointers valid for their declared use.
Concurrent unsynchronized writes to one instance or buffer remain invalid.

```text
@thread_safe
export def scale(value: f32) -> f32 = "audio_scale":
    value * 0.5
```

The first supported signatures contain numeric scalars, `bool`, and opaque pointers,
with scalar, opaque or unit results. Managed class handles, scalar loans and
aggregate results are not supported on annotated exports. They need a separate
transfer contract. Unannotated exports retain the serialized caller-thread
contract in the [generated C interface](exports.md).

Certification follows helpers, generic instances, callable targets and
reachable destructors. Fixed-symbol foreign calls must declare `@thread_safe`; opaque
pointers held by the body need `@thread_movable` or `@thread_shareable`.
Host-supplied pointers are checked as local values: the host, rather than a
Plenty move expression, supplies the lifetime and synchronization proof.
This does not authorize copying an ordinary movable pointer between Plenty
workers or weaken ordinary borrow checking.

For trusted raw memory access, the thread promise concerns independent memory
regions or properly synchronized accesses. An aliased pointer is not an
exclusive reference. A binding must not label a writable resource
`@thread_shareable` merely to pass checking. A movable pointer may be accessed
on different threads with ordered handoff, and separate pointees may be
accessed concurrently.

## Host protocol callbacks

A host can supply a table of native function addresses whose calls are valid
only during a particular protocol callback. A trusted binding can declare:

```text
@thread_movable
pub opaque Pointer
@host_callback
pub extern def count(function: Pointer, context: Pointer) -> u32 = function
```

`@host_callback` applies only to an indirect `extern def` with an opaque address
parameter as its target. It is mutually exclusive with `@thread_safe` and is
rejected on fixed symbols, intrinsics and exports. The binding author promises:

- the address is non-null, resident and has exactly the declared native ABI;
- the current host callback and thread permit this operation;
- host data and callback lifetimes, synchronization and reentrancy rules hold;
- borrowed arguments are not retained beyond the call;
- no native unwind or nonlocal jump crosses the Plenty frame.

The compiler trusts these promises; it cannot infer host protocol state from a
pointer. A certified export may reach these calls through helpers, generic
instances, thin callables or destructors. Its C callers must supply the required
host context: certification does not make the callback valid on arbitrary
threads. Ordinary Plenty workers, executor jobs and parallel callbacks still
reject such calls, including when they call an exported Plenty function.
Serial code retains its existing trusted-FFI obligations.

Generated import wrappers omit `@thread_safe` for exports that depend on this
permission, preventing ordinary workers from recovering permission through a
fixed-symbol wrapper. A host-context requirement appears in the C header and
generated interface contract, including its fingerprint. Exports with no such
dependency retain their existing wrapper certification.

## Runtime entry

The generated C adapter saves and restores diagnostic caller context in
thread-local storage. The runtime's retain counts are atomic, and destruction
queues and actor execution context are thread-local. The separate library runtime
uses the system allocator; embedded applications retain allocation caches backed
by synchronized shared storage. Library calls do not require a host thread to
have started through Plenty's `spawn`.

Libraries leave process signal handlers to their host. The runtime's executable
stack-overflow handler is not installed by a library entry; this interface does
not add recovery from stack overflow, traps, or explicit unwrap failures.
Foreign unwinding and nonlocal jumps across Plenty frames remain forbidden.
Code and static data must stay resident while any pointer or callback is live.
Actual unloading additionally requires the explicit [unloadable library profile](unloading.md) and its teardown contract.

The export checker reuses worker eligibility, which permits allocation, locks,
blocking operations and I/O. **Thread certification is not a real-time execution
guarantee.** No timing, allocation-free callback, or audio scheduling claim is
made. Native bindings remain trusted promises which the compiler cannot prove.

Generated Plenty import wrappers conservatively keep opaque pointer types
thread-bound, even when individual exported functions are certified. The C
header states each export's actual thread contract; wrapper thread mobility
needs additional representation of mixed entry-point contracts.
