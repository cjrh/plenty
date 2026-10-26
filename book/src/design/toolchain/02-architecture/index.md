# Compiler and runtime architecture

```text
source → lexer/parser → name and alias resolution → signatures → checking
                                                                  ↓
                                                        typed operation IR
                                                                  ↓
                                                     Cranelift → object → linker
                                                                           ↑
                                                           embedded Rust runtime
```

- [Frontend and checking](frontend.md): types, ownership, access CFGs, and IR.
- [Native emission](emission.md): specialization, batching, and performance evidence.
- [Value representations](values.md): inline storage, standard sums, and relocation.
- [Memory and destruction](memory.md): owners, reference counts, and allocator provenance.
- [Collections](collections.md): storage, hash order, and current costs.
- [Generators and closure environments](environments.md): frames and captures.
- [Tail-call ABI](tail-calls.md): approved transfers and hidden result storage.
- [Inspecting generated code](dump.md): `--dump` text for each function's IR and machine code.
- [Runtime packaging](runtime.md): archives, platform checks, stack overflow, and Rust panics.
- [Concurrent mechanisms](concurrency.md): pinned storage, locks, and waiters.
- [Native interfaces](interop.md): adapters, metadata, and resident code.
- [Support index](support.md): links to implemented contracts and major restrictions.

The opt-in [compiler phase profiler](../../../development/profiling.md) reports
lowering setup/body/finalization, ownership analysis subphases, and native
compilation/code collection/context cleanup. Benchmark workloads can be selected
independently; profiling is never activated by the feature alone.

These pages describe mechanisms. Source rules live in the
[language contract](../../language/01-overview/index.md), and operation guarantees
live beside the [built-in APIs](../../facilities/01-builtins/index.md).
