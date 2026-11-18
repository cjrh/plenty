# Package-declared allocators

**Proposal only; not implemented or scheduled.** Today ordinary allocating expressions use the system interface, and callers can choose a provider through [`std.alloc`](../design/stdlib/alloc.md). This proposal keeps both choices. It would let a package declare, at compile time, which allocator its ordinary allocating expressions use. No directive or manifest syntax is proposed here.

## Which package pays?

A parser author can pass a budgeted allocator through every helper, yet accidentally write `xs[1:4]?` and charge the system. A package declaration would let the author keep the slice expression and its visible failure path. The rule is lexical: **the package chooses where a new owner is created; the owner remembers how to grow and release its storage.**

Suppose `parser` declares a budgeted allocator and `app` declares none:

| Operation | Proposed storage policy |
| --- | --- |
| `parser` evaluates `xs[1:4]?` on a list supplied by `app` | The new list uses `parser`’s provider, including newly allocated element copies. Already-shared backing stays shared. |
| `app` grows or drops the list returned by `parser` | The returned list uses its retained provider. The site of growth or destruction does not replace it. |
| `parser` grows a list originally allocated by `app` | The existing list keeps its own provider. |
| `parser` calls `out.slice(&xs, 1, 4)?` | The explicit `out` wins, including when it is the system allocator. |
| `parser` borrows `&xs[1:4]` | No new owner and no allocation. |

Without a declaration, ordinary shorthand keeps its system policy. Neither form hides allocation failure: if the chosen provider refuses, return the allocation error; do not retry against the system. The declaration changes neither public owner types nor eligibility to borrow, copy or send a value to a worker.

A builtin expression belongs to its **source package**, even when a runtime helper does the allocation. Its result and required scratch follow the operation’s storage policy. Calling an ordinary dependency function is different: allocations written in that function follow the dependency’s policy, not its caller’s. A generic helper keeps its defining package’s policy after specialization or inlining. There is no dynamically scoped “current allocator,” process-wide hook or transitive budget for dependencies and foreign code.

## A declaration selects a provider, not a lifetime

A candidate starting point is one runtime provider instance per configured package per program, shared by its modules and workers under the existing provider safety rules. A compile-time declaration does not create that state by itself. Open questions include how it names or receives the provider, who supplies its bootstrap allocator, how initialization failure reaches the caller, and when the package releases its handle. Initialization must not recursively depend on the provider it is creating. An owner that escapes the package must keep its provider alive. This is a policy option, not a substitute for explicit handles when callers need separate budgets for concurrent requests.

Consider an 8 MiB bump arena used for a parser’s startup tables. It returns aligned blocks by advancing a cursor; exhaustion returns `AllocError.OutOfMemory`. Dropping a list still destroys its elements, but need not reclaim individual blocks. Growth may consume space for both the old and new buffers. Backing can be returned to the parent allocator when the last handle and owner are gone. This fits build-once data better than a long-running service repeatedly allocating temporary parse trees.

For per-parse work, pass a fresh arena explicitly or store it on a parser instance. An escaping tree can retain that arena; alternatively, copy the durable result to the caller’s allocator and release the scratch arena. A small retained result can otherwise keep the whole arena alive. Returning from a package function is **not** a reset point: a caller or worker may still hold a list or view. The proposal includes no public reset. Safe reuse would require proof or a check that nothing live depends on the old contents. The existing [experimental arena](../design/stdlib/experimental-alloc.md) is thread-confined, not a ready-made shared package provider.

## Precedents and alternatives

- **[Ada 2012: Default Storage Pools](https://ada-rapporteur-group.github.io/ARM/Ada_2012/RM-13-11-3.html).** A package or declarative region can name a default storage pool. The choice applies to eligible **access types declared there** unless the type specifies its own policy; it does not redirect every allocation expression in the package. A generic instance normally uses the generic definition’s default, with an explicit instance override available. Plenty would borrow the lexical principle but attach it to the allocating operation, not the public type.
- **[Odin: implicit context system](https://odin-lang.org/docs/overview/#implicit-context-system).** `context.allocator` lets code use ordinary allocation syntax, but the context propagates through procedure calls. A Plenty package declaration would deliberately *not* propagate into callees.
- **[Zig: choosing an allocator](https://ziglang.org/documentation/master/#Choosing-an-Allocator).** An allocating API typically takes an allocator value or uses one stored on an object. A raw returned slice carries a pointer and length, not the allocator needed to free it. Explicit passing remains a good Plenty option for caller-owned budgets; Plenty’s retained owners already solve release across package boundaries.

## Questions to test before adoption

Use the [allocation-site inventory](../design/toolchain/02-architecture/allocation-sites.md) to distinguish independent results and scratch from loans, transfers and growth. Prefer a diagnostic at an unsupported allocating site over silently bypassing the declaration. Existing allocator/actor restrictions and checked-stack guarantees would still apply; lexical shorthand must not hide provider effects from those checks.

A small future experiment would need only `app`, `parser`, and one list slice. Check which provider pays when the slice is inside a generic parser helper, when `app` grows the returned list, and when an explicit allocator overrides the default. Force refusal and verify that the source remains intact and no system allocation substitutes for the failed request. Those observations matter more than choosing directive syntax now.
