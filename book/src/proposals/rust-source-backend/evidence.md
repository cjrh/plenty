# Rust-source probe evidence and adoption criteria

This accompanies the [proposal](../rust-source-backend.md) and
[semantic contract](contract.md). Results are bounded evidence, not a completed
backend or a performance comparison.

The subsequent [checked-emitter spike](checked-emitter.md) adds actual checked-IR
generation and compile/link measurements. The hand-lowered ownership results
below remain separate evidence; they are not implemented by that scalar emitter.

## What is already generated

The [Rust dependency experiment](../rust-crate-dependencies/evidence.md) has
an actual restricted emitter in `experiments/rust-crates/run.py`. It checks
Plenty against declared API stubs, reparses a small accepted subset and emits
calls, borrows and checked arithmetic to Rust source. A real regex owner and
synthetic future execute through that path. That establishes feasibility of
source emission with native Rust owners; it does not consume checked compiler
IR or implement the language. Its earlier results were not rerun for this
proposal and should retain their original measurement qualifications.

## New semantic probes

`experiments/rust-source-backend/` contains dependency-free Rust candidate
implementations, a separate Rust-host integration test, paired real Plenty
programs and a runner. **The Rust is hand-lowered**, deliberately isolating
semantic questions from implementing a second parser or a production emitter.
Run `python3 run.py --plenty /absolute/path/to/plenty` from that directory.
Inputs, expected observations and the recorded snapshot are in its README and
`RESULTS.md`; machine-readable observations are written under ignored `target/`.

Tested on x86_64 Linux with Plenty 0.3.0 from main `00d32a13`, rustc 1.99.0
(`b940084d7`, LLVM 23.1.1), edition 2024. The local Rust checkout inspected for
Vec reservation and panic/abort implementation was `080cc27146389dbb5c8772c476b1937be9c6110f`,
distinct from the tested compiler revision. It was used as source evidence,
not built or assumed identical to the installed toolchain.

| Question | Observation | Limit |
| --- | --- | --- |
| Can Rust consume native-looking APIs? | Separate Rust test crate passes a native vector borrow, `Cursor` and consuming closure; returned element points into the original vector. | API declarations are handwritten, not inferred. No general Rust crate importer. |
| Do scope cleanup and propagation align? | Both ordinary/debug and release Plenty/Rust print `33, 2, 1, 3, 4`: numeric result, reverse lexical cleanup on `?`, explicit moved-owner drop, final marker. | One concrete control-flow path; not closure/generator/recursive cleanup coverage. |
| Does checked overflow preserve fatal behavior? | Explicit checked-add/abort fails in both modes with no destructor output, as Plenty does. | Trap formatting, exact exit status, stack guard and source attribution are not reproduced. |
| Can ordinary Rust panic stand in for a trap? | No: default panic prints destructor trace `99`; separate `panic=abort` build does not. | Does not qualify arbitrary dependencies, callbacks or unwinding Rust hosts. |
| Does native fallible reservation help? | Impossible capacity fails before any element callback; normal construction succeeds. | Capacity overflow only; no allocator fault injection or Plenty owner/error representation parity. |
| Can all admitted loans be printed literally? | No: distinct constant list-index loans pass Plenty but naive Rust fails E0499; safe split translation passes. | Only a two-element constant-index case. |
| Does Rust still check owner misuse? | Moved-vector access is rejected with E0382. | Redundant checking cannot replace Plenty checks or prove the emitter correct. |

Two library tests and one Rust-host integration test pass in debug and release.
The runner checks four differential executions, two rejected Rust sources and
three panic observations. Cargo formatting and Clippy with warnings denied pass.
No production compiler source changed, no external dependencies were added, and
no cross-target or performance result is claimed.

## Decision gate for a first backend slice

The [checked-emitter experiment](checked-emitter.md) now satisfies the scalar
part of the original gate: real checked-frontend emission, native companion
calls, source mapping and build measurements. The remaining ownership-bearing
gate is to admit simple move-only scalar records, named enums/Results,
structured branches/loops, local borrows and a returned first-parameter borrow.
Expose monomorphic exports and a small explicitly declared Rust companion API.
Exercise a genuine Rust host that calls an exported Plenty function which in
turn calls that companion. Restrict recursion and resource features until their
contracts have a translation; unsupported cases must fail explicitly.

The adoption decision should require all of the following evidence:

- Differential tests against Cranelift for admitted semantics, including
  overflow, early exits, drop order, moves, aliases and fault-injected allocation
  if owned dynamic storage is admitted.
- A public Rust API usable without unsafe calls or per-call marshalling glue,
  with documented ownership and panic behavior and no hidden copy workaround.
- Source-located Plenty diagnostics for unsupported inputs, plus at least one
  generated rustc error mapped back to the triggering Plenty source.
- Cold/warm/edit build and representative runtime measurements, separately
  attributing source generation, Rust checking/optimization, dependency builds
  and linking. Compare the same accepted workloads, not tiny warm Cargo no-ops.
- A documented list of retained frontend facts, support-runtime dependencies,
  unsafe helper invariants if any, and costs of keeping both backends correct.

If useful native APIs require broadly changing Plenty's semantics or pervasive
unchecked lowering, stop expansion and prefer the generated-adapter route.
If safe monomorphic library exports work with a small support layer, that alone
can be worthwhile without committing to a whole-language compiler replacement.
This gate describes the technical decision; scheduling remains in
[B20](../../backlog.md).
