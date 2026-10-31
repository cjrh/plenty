# Arbitrary-precision integers (`std.bigint`)

**Status: Not implemented.** This page compares ways to provide issue #481's
`std.bigint` and recommends one. It ships no module, compiler change or API.
Scheduling belongs to [B38](../backlog.md). The current integer contract is
[numeric types](../design/language/03-types/numbers.md); library operators are
[user-defined arithmetic operators](../design/language/04-expressions/operators.md).

## Recommendation

Write `std.bigint` in Plenty, in three steps:

1. **Plenty module.** A `BigInt` with an inline `i64` for small values and
   `u64` limbs for large ones. Schoolbook and Karatsuba multiplication, Knuth
   division and divide-and-conquer radix conversion, ported from `num-bigint`
   (MIT OR Apache-2.0) with credit. Every allocation is an ordinary fallible
   Plenty allocation.
2. **Compiler builtins for limb arithmetic.** `u64.widening_mul` (Cranelift
   `umulhi` plus `imul`) and an add-with-carry form. The measured limb loop is
   4.6× slower than Rust without them.
3. **Optional native kernel, only if measured to pay.** A runtime entry for
   large multiplication (Toom-3 or an FFT) that writes into an output and
   scratch slice the Plenty caller has already allocated. It allocates nothing
   itself, so it needs no allocation-failure handling. Write it in permissively
   licensed Rust; do not use malachite.

**Malachite does not fit.** On allocation failure it aborts the process. It is
LGPL-3.0-only, and the runtime archive is statically linked into user
executables. It adds 1.2 MB to every executable that uses it and 35 s to a
clean build. Its multiplication beats the alternatives only above about 10⁴
bits, and only through its AVX2 FFT on x86-64; the portable path was slower
than every other crate, and ARM64 was not measured. It also parses decimal text
2–6× faster.

## Requirements

- **Fallible allocation.** Every allocation in `std` reports `AllocError`; a
  Rust allocation failure in the runtime aborts the process
  ([Rust panics](../design/toolchain/02-architecture/runtime.md#rust-panics)).
- **License.** Plenty is MIT OR Apache-2.0. No current dependency is used under
  a copyleft license. The only license expressions that mention LGPL also
  offer MIT OR Apache-2.0.
- **Size and build cost.** Code in the runtime archive costs nothing to programs
  that do not reach it, but grows the compiler and its build. The
  [engine criteria](../design/toolchain/01-building/engines.md#when-a-module-is-an-engine)
  move a module out of the archive above 2 MB per program or one minute of
  build time.
- **Platforms.** Linux x86-64 and ARM64, macOS ARM64, Windows x86-64 MSVC
  ([cross-platform assessment](cross-platform.md)).
- **Project preference.** Implement in Plenty unless native complexity or
  security justifies a wrapper (B38, `AGENTS.md`).

## Allocation failure in candidate crates

Each crate's source was read and run with a global allocator that refuses
allocations over 4 KiB, then asked to square a 65,536-bit number.

| Crate | License | How it allocates | On failure |
|---|---|---|---|
| `malachite-nz` 0.14.0 | LGPL-3.0-only | `Vec` throughout (about 1,500 `vec!`/`push`/`with_capacity` sites, no `try_reserve`); the FFT allocates internally | `handle_alloc_error`: abort |
| `num-bigint` 0.5.1 | MIT OR Apache-2.0 | `Vec<u64>`, no `try_reserve` | abort |
| `ibig` 0.3.6 | MIT OR Apache-2.0 | `alloc::alloc` in `memory.rs`/`buffer.rs`, null handled by `handle_alloc_error` | abort |
| `dashu-int` 0.6.2 | MIT OR Apache-2.0 | `alloc::alloc` in `memory.rs`/`buffer.rs`; null calls `panic_out_of_memory()` | panic, which is an abort in the `panic=abort` runtime |
| `rug` 1.30 / GMP 6.3 | LGPL-3.0+ / LGPL-3.0 or GPL-2.0 | `mp_set_memory_functions` | GMP's manual: allocation functions "must terminate program execution. A `longjmp` or throwing a C++ exception will have undefined results." |

No crate exposes an allocator parameter or a fallible API. Malachite's
limb-level functions with caller-supplied scratch, such as
`limbs_mul_greater_to_out`, are crate-private unless the `test_build` feature
is enabled, and its FFT path allocates regardless.

### Can the runtime's arena catch the abort?

The runtime can already contain one infallible Rust library: `ring`'s RSA
operations run inside `actor_heap::within` with a pre-reserved arena. An
allocation failure there jumps back and reports `AllocError`
([RSA allocation](../design/stdlib/crypto-keys.md#allocation-and-key-material)).
Applying this to malachite would fail for these reasons:

- **Platforms.** The arena router exists only on Linux x86-64 and ARM64. On
  other targets `within` runs the job directly and a failure aborts.
- **Value-dependent memory.** `ring` uses a fixed amount of memory per
  operation, so its arena has a fixed, tested size. A bignum operation's
  memory grows with its operands (the FFT path uses O(n log n)), so each call
  must estimate a bound and reserve it first.
- **Results cannot leave the arena.** `within` forbids handing back
  allocations. Every result `Natural` would have to be copied into Plenty
  storage before the arena is freed: an extra O(n) copy and twice the peak
  memory.
- **dashu panics** instead of calling the allocation-error handler, so the
  arena cannot catch its failures at all.

An engine (shared library) does not help either: Rust's `Vec` aborts there
too, and the [linalg engine](../design/stdlib/linalg-engine.md#allocation-contract)
reports allocation failure only because it uses fallible faer APIs and a fork.
Forking malachite (about 650,000 lines with `malachite-base`) to make it
fallible is not maintainable.

## License: LGPL-3.0 in a statically linked runtime

The runtime archive, including any Rust crate it wraps, is statically linked
into each executable that reaches the module. Under LGPL-3.0 §4, everyone who
distributes such an executable conveys a "Combined Work" and must:

- give prominent notice and ship the GPL-3.0 and LGPL-3.0 texts;
- use terms that do not restrict modification of the library or reverse
  engineering to debug such modifications;
- either supply the application in a form the recipient can relink with a
  modified malachite (object files or source), or load malachite through a
  shared-library mechanism;
- for "User Products", supply installation information.

These obligations fall on every Plenty user who distributes a binary that uses
`std.bigint`, not only on the Plenty project. Relinking is also hard in
practice: the Rust code is monomorphized and inlined into the runtime archive,
so the user would need the whole runtime archive rebuilt against the modified
malachite. A shared engine (`libplenty_bigint_v1.so`) would meet the
relinking condition with §4(d)(1), but it still leaves the allocation-failure
problem, costs a dynamic call per operation and must be shipped with the
application. GMP/`rug` raises the same issues. GMP also needs `m4` and `make`
to build and does not support Windows MSVC (MSYS2/MinGW only).

`num-bigint`, `ibig` and `dashu` are MIT OR Apache-2.0, so their algorithms can
be ported into Plenty with credit in the module header and its
`## Acknowledgements` section.

## Measurements

Measured 2026-10-31 on an AMD Ryzen 7 7840HS, rustc 1.99.1, release build with
`panic=abort` and `codegen-units=1`, pinned to one core. Inputs are
pseudo-random. The scratch crate lives under the worktree's `target/` directory
and is not committed.

### Throughput

Nanoseconds per operation. "Small" is 10 million `&a + &b` or `&a * &b` on
values that fit in a word, producing a new value each time.

| Operation | num-bigint | malachite | dashu | ibig |
|---|---:|---:|---:|---:|
| small add | 8.2 | 7.3 | 7.2 | 1.1 |
| small multiply | 7.6 | 5.8 | 6.0 | 5.1 |
| multiply 256 bits | 30 | 33 | 48 | 29 |
| multiply 4,096 bits | 2,296 | 1,947 | 1,730 | 2,129 |
| multiply 65,536 bits | 175,398 | 59,958 | 145,030 | 154,566 |
| multiply 1,048,576 bits | 11,081,585 | 1,891,493 | 7,316,193 | 9,840,681 |
| 20000! by repeated multiply | 21.8 ms | 26.0 ms | 24.2 ms | 19.8 ms |
| format 20000! (77,338 digits) | 3.7 ms | 2.4 ms | 3.2 ms | 3.5 ms |
| parse 20000! | 4.2 ms | 0.70 ms | 1.5 ms | 1.7 ms |

Malachite is 2.4–6× faster at 65,536 bits and above, and much faster at
parsing. Up to 4,096 bits and on small values it is no faster than the others.
Its large-size speed comes from an f64 FFT that uses AVX2 and FMA only when
the `std` feature enables runtime CPU detection. With `default-features =
false` (the portable path), 65,536-bit multiplication took 837,473 ns, slower
than every other crate. The ARM64 path uses the `wide` crate and was not
measured.

### Plenty limb arithmetic

The same schoolbook multiplication was written in Plenty, compiled with
`--release`. Plenty has no `u128` or high-half multiply, so each 64×64-bit
product uses four 32-bit multiplies. The Rust baseline uses `u128`.

| Size | Rust u128 schoolbook | Plenty, product in a helper function | Plenty, product inlined by hand |
|---|---:|---:|---:|
| 256 bits | 23 ns | 403 ns | 357 ns |
| 4,096 bits | 4,879 ns | 33,705 ns | 22,389 ns |
| 65,536 bits | 929,096 ns | 7,814,643 ns | 4,878,989 ns |

The inlined inner loop costs 5.5 ns per limb product against Rust's 1.2 ns. The
small-size cost is mostly fixed overhead: allocating and filling the result
list and the call. Holding 32-bit limbs in `u64` slots, so that each product
needs one multiply, was slower (43,463 ns at 4,096 bits). The remaining cost is
in the checked adds and the indexing, not the multiply. A widening-multiply
builtin and a carry form would remove the emulation and most of the overflow
checks.

A Plenty small-value fast path is as fast as the crates' small paths:
`enum Big: Small(i64) | Large(list[u64])` with an `__add__` hook that tries a
`checked:` add takes 8.7 ns per `(acc + one)?`. Calling a native crate would
add a call into the runtime and an owned native handle per result, on top of
the crate's own 6–8 ns.

### Executable size and build time

The program parses one number, squares it and prints it. Sizes are of a
stripped Rust executable; the baseline prints its argument.

| Crate | Executable | Added | Clean build, 8 jobs |
|---|---:|---:|---:|
| none | 339,416 B | — | — |
| num-bigint | 411,912 B | +72 KB | 2.4 s |
| ibig | 408,856 B | +69 KB | 2.5 s |
| dashu-int | 472,400 B | +133 KB | 4.5 s |
| malachite-nz (`std`) | 1,543,816 B | +1.20 MB | 35 s |

Malachite would remain under the 2 MB engine threshold. Its build time would
use more than half of the one-minute budget, and every compiler build would
pay it. A Plenty module adds only the functions a program reaches; it needs no
Rust build.

## Options compared

| | Plenty module (+ builtins, + optional kernel) | Wrap malachite | Wrap num-bigint / ibig / dashu | GMP via `rug` |
|---|---|---|---|---|
| `AllocError` on failure | Yes, on every target | No; arena on Linux only, with copy-out and per-call bounds | No; same arena limits; dashu cannot be contained | No (undefined if unwound) |
| License | MIT OR Apache-2.0 (ported, credited) | LGPL-3.0-only obligations on every distributor | Permissive | LGPL/GPL, same obligations |
| Size per program | Only reached functions | +1.2 MB | +70–130 KB | GMP static library |
| Compiler build | None | +35 s | +2–5 s | C build with `m4` |
| Platforms | All targets | All, AVX2 FFT on x86-64 only | All | No Windows MSVC |
| Small values | 8.7 ns measured, no allocation | Native call + handle + 6–8 ns | Same | Same |
| Large values | Slower until builtins/kernel; 4.6× behind Rust schoolbook today | Fastest above 10⁴ bits | Comparable to each other | Fastest |

## API direction

These signatures are a sketch for the implementation PR to settle.

```text
from std.bigint import BigInt

class BigInt            # not a copy class; copy(x)? duplicates the limbs
    def __add__(self, other: &BigInt) -> Result[BigInt, AllocError]
    def __radd__(self, other: i64) -> Result[BigInt, AllocError]  # i64 + BigInt
    def __sub__, __mul__, __floordiv__, __mod__, __neg__, __lshift__, __rshift__,
        __and__, __or__, __xor__   # same shape
    def add_assign(self: &mut BigInt, other: &BigInt) -> Result[(), AllocError]
    def cmp(self, other: &BigInt) -> i64          # until __cmp__ (B37)
    def to_i64(self) -> Result[i64, RangeError]
    def to_f64(self) -> f64
    def display[W](self, out: &mut Formatter[W]) -> Result[(), FormatError]
    def to_str(self, radix: i64) -> Result[str, AllocError]
def from_i64(value: i64) -> BigInt                # no allocation
def parse(text: &str, radix: i64) -> Result[BigInt, ParseError | AllocError]
```

- **Ownership.** Operator hooks take a shared `self` and a shared or copyable
  operand, and return an owned result. `a + b` borrows both operands and never
  consumes them. A result that fits in `i64` is stored inline and allocates
  nothing, but the hook still returns `Result`, so callers write `(a + b)?`.
- **In-place operators.** `__iadd__` must return `()`, so it cannot report
  `AllocError`. Provide named fallible methods (`add_assign`) instead, which is
  the reference's own advice ("use explicit assignment or a named fallible
  method").
- **Division.** Use floor semantics, as `//` and `%` have on `i64`. A zero
  divisor traps, as it does for `i64`. A checked variant can return
  `ZeroDivisionError`.
- **Equality and ordering.** `==` is structural, so it is correct only if the
  representation is canonical. Every value that fits in `i64` is `Small`, and
  limbs carry no high zeros. Classes have no ordering operators until the
  [user-defined ordering](user-type-ordering.md) proposal's `__cmp__` lands, so
  `cmp` is a method for now. `BigInt` cannot be a dictionary key until
  [user-type hashing](user-type-hashing.md) lands.
- **Limits.** Text conversion of attacker-controlled input should either use
  subquadratic algorithms or take a digit limit, like other parsers' `Limits`.

## Open questions

- `BigInt + i64`. Each operator has one left hook, so `__add__` cannot accept
  both `&BigInt` and `i64`. `i64 + BigInt` works through `__radd__`. For the
  other order, callers write `a + from_i64(n)`, which allocates nothing,
  unless hooks gain overloading or a mixed-operand sum type.
- Names and signatures of the limb builtins (`widening_mul -> (u64, u64)`,
  `carrying_add(other, carry: bool) -> (u64, bool)`), and whether Cranelift
  lowers them on ARM64 as cheaply as on x86-64.
- The size threshold above which a native multiplication kernel is worth its
  code, measured after the builtins land.
- Whether small helper functions in the limb loop need inlining. The measured
  call to `mul_wide` cost 1.5× in the 4,096-bit loop.

## Acknowledgements

- [`num-bigint`](https://github.com/rust-num/num-bigint) (MIT OR Apache-2.0):
  the reference implementation suggested by issue #481; the recommended source
  for ported algorithms.
- [`malachite`](https://www.malachite.rs/) (LGPL-3.0-only),
  [`ibig`](https://github.com/tczajka/ibig-rs) and
  [`dashu`](https://github.com/cmpute/dashu) (MIT OR Apache-2.0), and
  [GMP](https://gmplib.org/) (LGPL-3.0 or GPL-2.0): evaluated and measured
  here; no code is used.
