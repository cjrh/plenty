# Boxed protocol research evidence

This records the investigation behind [boxed protocols](../boxed-protocols.md).
The inspected baseline is local `main` at `bb97038b`, dated 2026-11-03. The
local debug compiler reported `plenty 0.8.8 (x86_64-unknown-linux-gnu)`.
No protocol-object implementation or performance measurement was produced by
that original investigation. The current implementation is described in the
[reference](../../../design/language/07-generics/boxed-protocols.md); the harness
now tests protocol objects by default. Use `--baseline` with the research-era
compiler to reproduce the historical rejection below.

## Implementation measurements

The implementation was measured on x86_64 Linux with optimized native output
(`--release`). The retained `scripts/probes/boxed_protocols_bench.py` constructs
20,000 alternating one-field/two-field objects, performs 200 shared-read passes
(4 million calls), and destroys the list. Each phase uses `std.time`; the table
reports medians of five runs in milliseconds, excluding process startup.
Construction includes list growth. Every representation checks the same checksum.

| Representation | Construction | Shared reads | Cleanup | Executable bytes |
| --- | ---: | ---: | ---: | ---: |
| inline enum | 1.870 | 7.731 | 0.272 | 183,896 |
| concrete box | 2.345 | 7.522 | 0.587 | 184,712 |
| boxed callable | 3.501 | 22.213 | 0.903 | 186,344 |
| boxed protocol | 2.024 | 16.210 | 0.689 | 185,112 |

The protocol representation trades indirect-call cost for an interface that can
accept independently supplied implementations. In this workload its shared-read
phase takes about twice the inline-enum/concrete-box time, and less than the boxed
callback adapter. These are local microbenchmarks, not general speed promises:
payload sizes differ between representations, methods do very little work, and
host load changes the results. Mutation performance, large method tables and
other native targets were not measured. Native execution qualification here is
x86_64 Linux; no macOS, Windows or AArch64 run is claimed.

The harness also reports one cold compiler invocation per representation and
linked executable size; compiler timings are not repeated measurements. Run it
with a compiler built from this checkout:

```sh
python3 -B scripts/probes/boxed_protocols_bench.py target/debug/plenty
```

## Reproducible compiler probes

The retained harness is `scripts/probes/boxed_protocols.py`. It writes isolated
temporary programs, disables the compiled-function cache with `PLENTY_CACHE=off`,
executes positive cases and checks expected diagnostic text in negative cases.
It requires no third-party Python packages.

From the checkout whose behavior is being investigated:

```sh
cargo build --locked --bin plenty
python3 -B scripts/probes/boxed_protocols.py --baseline target/debug/plenty
```

The investigation ran the harness with the existing debug compiler in the
primary checkout; no compiler/runtime sources differ between that baseline and
this proposal branch. All **four probes passed**: two native programs and two
expected type-checking failures. The rejection probes characterize this
baseline, not permanent requirements; update them when B29 implements boxing.

| Probe | Observed result | What it establishes |
| --- | --- | --- |
| Declare `items: list[Box[MyProtocol]] = []?` | Rejected: `unknown type MyProtocol` (the diagnostic quotes the name with backticks). | Protocols currently do not resolve as boxed value types. |
| Iterate over `list[Box[A]]` and call `item.hey()` | Runs; prints `10`. | Concrete boxes already support method calls through borrowed list elements. |
| Append `Box[B]` to that `list[Box[A]]` | Rejected: `expected Box[A], got Box[B]`. | Heap storage alone does not erase the concrete type. |
| Adapt a class and an enum satisfying `MyProtocol` into boxed callbacks | Runs; prints `10` then `20`. | Existing generic conformance, owned capture and callback erasure compose for one shared method. |

Both implementations used by the probes:

```text
protocol MyProtocol:
    def hey(self) -> i64:
        pass

class A:
    value: i64

    def hey(self) -> i64:
        self.value

enum B:
    Value(i64)

    def hey(self) -> i64:
        match self:
            case B.Value(value):
                *value
```

The positive erasure probe adds this current-language code:

```text
def erase[T: MyProtocol](value: T) -> Result[BoxedCallable[[], i64], AllocError]:
    callback = def [value]() -> i64:
        value.hey()
    BoxedCallable(callback)

def main() -> Result[(), AllocError | IoError]:
    mut items: list[BoxedCallable[[], i64]] = []?
    items.append(erase(A(10))?)?
    items.append(erase(B.Value(20))?)?
    for item in &items:
        print(item())?
    Ok(())
```

The protocol requirement is checked when specializing `erase`. The resulting
callbacks have the same public signature despite different concrete captured
values. Calling the callback reaches the appropriate `hey` implementation.

This does **not** test multiple named methods, mutable erased receivers,
protocol-table generation, `Box[P]` satisfying a generic bound, destruction
under injected allocation failure, or returned borrows. Separate callbacks
capturing separate objects would not establish that several methods operate on
the same state. Existing `BoxedCallable` also rejects mutable and consuming
callbacks; the successful probe does not remove that limit.

## Source findings

| Finding at the baseline | Source evidence | Consequence for the proposal |
| --- | --- | --- |
| Protocols are stored as required method declarations. | `src/frontend/protocols.rs`, `Protocol` and `Parser::protocol_decl` | Reuse declarations and exact conformance checks; do not assume a runtime protocol type already exists. |
| The protocol's own name substitutes to the implementing type. | `protocols::check` inserts the concrete type into its alias environment. | Preserve a marker long enough to reject non-receiver occurrences before erasure. |
| Protocol conformance checks accessible methods and receiver/signature equality. | `protocols::check`, including generic-requirement matching | Boxing must not weaken static guarantees or bypass module access. |
| Ordinary `Box[T]` requires a resolved concrete content type. | `src/frontend.rs`, `TypeRef::resolve` and the `Box` constructor path | Both annotation resolution and explicit protocol construction need new handling. |
| The IR has `Box`, `BoxedCallable` and `Callable`, but no protocol-object type. | `src/op.rs`, `Ty`, `BoxedCallableNew`, `BoxedCallableCall` | A distinct erased type/operation path is the safer starting point. |
| Boxed callbacks generate concrete receiver adapters. | `src/frontend/callables.rs`, `Lower::boxed_callable` | Multi-method objects can reuse the adapter approach without allocating one callback per method. |
| Calls use a hidden environment address and a fixed public signature. | `src/codegen/closures.rs`, `lower_boxed_callable_call` | There is already native indirect-call support; the proposed extension selects an entry from a static table. |
| The allocated box retains its concrete descriptor. | `plenty-runtime/src/aggregates.rs`, `Boxed`, `try_box_new`, `box_destroy`, operation 128 | Erased callers need not know layout to preserve correct relocation and destruction. |
| Metadata emission supports function-address relocations. | `src/codegen/metadata.rs`, `define` | Method tables can be immutable native data with adapter addresses. |
| Boxed callable eligibility and call signatures are independently checked. | `src/op/check.rs`, `BoxedCallableNew`/`BoxedCallableCall` arms | New protocol operations need equivalent validation, especially for mutable receivers. |
| Erasure does not certify worker eligibility. | `src/threading.rs`, `Ty::BoxedCallable` rejection | Preserve conservative nested transfer restrictions until a capability contract exists. |
| Shared-cycle checks treat mutable paths containing erased callbacks specially. | `src/frontend/shared.rs`, `rejection` | Hidden protocol payloads require the same safety audit. |
| Indirect calls affect ownership/effect summaries and function reachability. | `src/ownership/calls.rs`, `src/ownership/futures.rs`, `src/codegen/reachability.rs` | A callable table is not merely a runtime representation change. |

The box layout contains a word used for the boxed callback's adapter; its payload
slot begins at byte 32 in the inspected implementation. Those are current source
facts, not language promises. The suggestion to store a protocol-table pointer
there is an inference requiring its own runtime and target tests.

## Existing tests and local design history

`tests/test_boxed_callables.rs` already covers heterogeneous collections and
factory returns, generic callable constraints, shared calls, owned/borrowed
arguments, inline results, rejection of unsupported modes, ownership and
destruction. Runtime tests in `plenty-runtime/src/tests.rs` include
`callable_boxes_preserve_adapter_concrete_descriptor_and_relocated_captures`
and the allocation-checks test
`failed_callable_box_drops_the_concrete_environment_once`.
These tests were inspected, not rerun as part of this investigation; their
existence is a useful implementation template, not new validation evidence.

Local issue **#25** and merged local PR **#39** record the shipped
`BoxedCallable` work. The issue is closed; its resolution names merge commit
`b25a2309368070c90fb69a42a4ccbc1480857717` and implementation commit
`bc6b1f79d7bb4959671fde60c75c4ff46d91bdf5`. Its description and PR explain
explicit allocation, concrete descriptor retention, and deliberately excluded
mutable/consuming/borrowed/worker-crossing modes. These numbers refer to the
shared local `issues.db`, not remote records.

The [original protocol design](../protocols-generics-and-context-managers.md)
explicitly deferred existential interface values and called out method tables,
ownership and restrictions on implementing-type/generic signatures. The
[error-composition investigation](../error-composition.md#does-an-error-protocol-solve-it)
separates static bounds, finite error sums and erased error objects; allocation
while handling allocation failure remains a particular concern.

The [boxed callable reference](../../../design/language/06-functions/05-boxed-callables.md)
is authoritative for the shipped erasure contract. The
[protocol reference](../../../design/language/07-generics/protocols.md) confirms
that arbitrary anonymous sums do not forward user protocols, while
`std.fmt.Formattable` has specific sum forwarding. Its writer-generic method is
documented in [reporting](../../../design/stdlib/report.md#formatting-contract).
Do not infer general protocol-object support from that special case.

Semantic search was checked fresh against the primary checkout before drafting.
Related hits were verified against current source or local records. Some old
HTTP route discussions predate enum methods; their historical limitations are
not evidence that current enums lack methods. The native class/enum probe above
establishes the relevant current behavior directly.

## Confidence and limits

The feasibility conclusion is supported by current type checking, successful
single-method erasure, and inspected ownership/runtime mechanisms. It does not
establish a finished mutable protocol-object ABI, safe capability transfer,
cross-platform qualification, code-size impact, or dispatch performance.
The [qualification criteria](validation.md) distinguish those obligations from
the four baseline probes. No compiler/runtime feature has shipped in this work.
