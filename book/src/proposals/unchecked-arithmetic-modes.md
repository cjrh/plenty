# Unchecked arithmetic modes

> **Status: Partly implemented.** `--unchecked trap|assume` and
> `unchecked trap:` are implemented; see the
> [numeric reference](../design/language/03-types/numbers.md#unchecked-blocks),
> which is authoritative. A `plenty.toml` setting and the stdlib precondition
> audit remain under **B14** in the [backlog](../backlog.md); until that audit,
> `std` always traps. Issue #764 also extends the mode to list, slice-view
> and `Bytes` index syntax. This builds on
> [checked arithmetic by default](completed/checked-by-default.md).

## Contract

Plain `unchecked:` transfers responsibility for arithmetic preconditions to the
programmer. A compiler setting selects whether those preconditions are checked
at runtime or assumed valid. Trapping is the initial default in every build
profile; selecting optimization alone must not enable undefined behavior.

| Source form | Global trapping mode | Global assumed-valid mode |
| --- | --- | --- |
| Ordinary arithmetic | Declared recoverable arithmetic contract | Same |
| `checked:` | Local recoverable arithmetic contract | Same |
| `unchecked:` | Check arithmetic and trap on violation | Assume arithmetic preconditions; omit their checks |
| `unchecked trap:` | Check arithmetic and trap on violation | Check arithmetic and trap on violation |

Both unchecked forms produce an ordinary success value, without adding
arithmetic errors to the function's return type. `unchecked trap:` makes
termination on an arithmetic violation a permanent source-level guarantee.
Plain `unchecked:` must not promise termination independently of configuration.
The diagnostic mode catches violated obligations without changing signatures;
passing that mode's tests is evidence, not proof for all possible inputs.

In assumed-valid mode, overflow is **not allowed** and is **not wrapping**.
Violating an arithmetic precondition is undefined behavior: the compiler may
remove branches or transform loops on the assumption that violation never
happens. Silent wrong results and memory corruption are possible; a crash is
not guaranteed. An actual machine instruction wrapping does not define the
source expression's result. Use explicit wrapping operations for modular
arithmetic; explicit wrapping, saturating and checked methods retain their
own contracts in either mode.

## Overflow obligations

The no-overflow obligation applies uniformly to signed and unsigned integers.
Every intermediate result of ordinary integer arithmetic must be representable
in that operation's type, not merely the final result of the expression.
For example, `a * b // c` violates the contract if `a * b` overflows even when
the final mathematical quotient fits. Use a wider type, an algorithm that avoids
the overflowing intermediate, or explicit wrapping when modular arithmetic is
appropriate to the algorithm.

| Arithmetic contract | Overflow detection required? | May assume overflow never occurs? |
| --- | --- | --- |
| Explicit wrapping operation | No | No |
| Ordinary arithmetic in assumed-valid `unchecked:` | No | Yes |
| Ordinary arithmetic in trapping `unchecked:` or `unchecked trap:` | Yes, unless proven unnecessary | Only on paths where checking or proof establishes it |

Wrapping already permits arithmetic without overflow checks. Undefined overflow
adds optimization freedom: for example, the compiler may treat `x + 1 > x` as
true when the addition carries the no-overflow obligation. This is not a valid
way to detect overflow after the operation. The benefit beyond check omission
depends on the backend and workload; the source contract does not guarantee a
speed improvement over wrapping.

## Arithmetic scope and boundaries

The operation contract must specify which preconditions a block transfers:
representable integer addition, subtraction, multiplication, negation and
powers; valid integer divisors and representable quotients; valid shift counts;
and the existing validity conditions of numeric conversions and integer power
exponents. Preserve Plenty's existing successful arithmetic semantics, including
floor division and modulo. Do not import a machine instruction's different
semantics or forbidden cases as language rules. In particular, a defined signed
minimum remainder by minus one must remain zero if that is the existing contract.

IEEE floating-point arithmetic retains its existing behavior, including NaN and
infinity. Float-to-integer conversion retains its existing valid-input rounding
rules; its range/validity obligations must be specified separately from ordinary
floating arithmetic. Compile-time invalid constants remain compile errors.
No new recoverable shift or cast API is implied by this proposal.

Issue #764 extends the implemented mode to list, slice-view and `Bytes`
index syntax: plain assumed suites require nonnegative, in-range indices and
omit bounds checks and negative-index normalization. Trapping suites preserve
normal negative indexing. Other indexing and collection methods remain checked.

Allocation failure, capacity/layout validation for memory allocation,
ownership, borrowing, I/O failures and explicit `unwrap()`
are outside this feature. Memory-management arithmetic must still establish
its own safety invariants. The global mode is not permission to skip allocator
validation or to assume an allocation succeeded.

Modes apply lexically to arithmetic, not dynamically across calls. The nearest
explicit arithmetic suite controls an operation; leaving it restores the outer
mode. A nested plain `unchecked:` uses the global setting, even inside
`unchecked trap:`. A nested `checked:` restores local recovery. Named functions
and closures keep their independent contracts; merely defining or invoking one
inside an unchecked suite does not change its arithmetic mode. Inlining must
preserve these boundaries. `?`, returns, loop exits and ordinary Result values
keep their existing meanings.

The setting must have explicit, documented CLI/package configuration and
precedence. Its spelling is an implementation decision. It must participate in
compiled-artifact and function-cache identities, including dependency builds,
so changing the setting cannot reuse code with the opposite contract. Prebuilt
libraries retain their compiled semantics; a caller setting cannot retroactively
change them. A source `unchecked trap:` guarantee must survive all build modes,
worker lowering, specialization and cross-module optimization.

## Standard library rationale

Validate public inputs, then use a small plain unchecked region only where the
validated inputs and maintained invariants establish its preconditions. If a
block intentionally rejects invalid input by trapping, express that guarantee
with `unchecked trap:` or explicit validation. Merely enclosing existing code
in an unchecked suite does not establish a bound.
Classify each audited operation by intent: established bounds permit plain
`unchecked:`, intentional modular arithmetic requires explicit wrapping, and
invalid-input rejection requires `unchecked trap:` or explicit validation.

Review arithmetic opt-outs already introduced by checked-default adoption before
enabling assumed-valid mode for the stdlib. Preserve resource-failure mapping,
earlier successful mutations, callback error propagation and ownership cleanup
on defined paths. Use the repository's consistency-check procedure for a
repository-wide audit; proposal examples and historical fixtures must be
identified separately from executable code.

## Validation rationale

The modes need independent evidence. Invalid arithmetic belongs in trapping-mode
tests, which must check the trap and source location without widening signatures.
Never execute undefined arithmetic in assumed-valid mode to assert a particular
crash, wrapped value or output. Its runtime tests use valid boundary inputs;
static IR/native inspection establishes check omission and supported assumptions.
Explicit trapping suites need invalid-input tests in both global modes.
Cover signed and unsigned overflow obligations, including an overflowing
intermediate with a representable final mathematical result, in trapping tests.
Test explicit wrapping operations at overflowing inputs in both global modes
and both unchecked block forms: those operations remain defined.

Check nested modes, callbacks, generators, compound writes, short-circuit paths,
integer widths, divisors, shifts, conversions, powers and mode-sensitive cache
reuse. Verify that allocation, indexing outside assumed suites and explicit checked/wrapping operations
retain their contracts. Compiler backends must preserve valid-input semantics;
omitting guards alone does not establish that all desired optimizer assumptions
are represented.

Follow the [emitted-code procedure](../development/emitted-code.md): isolate small
kernels, compare equivalent C contracts, attribute remaining instructions and
measure optimized builds by instruction count. The programmer's explicit
precondition is the premise for removing a check in this feature; ordinary safe
code still needs a compiler proof. Report backend limitations and measured
results rather than promising that unchecked code is always faster. This
contract is permission to optimize, not a speed guarantee.
