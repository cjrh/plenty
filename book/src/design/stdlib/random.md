# std.random

`import std.random` provides deterministic pseudo-random generators that the
caller creates, seeds, owns, and passes explicitly, with one error type,
`RandomError`.

The module is checked Plenty code with no native primitives. It never reads
the operating system, the clock, or any process-wide state: there is no global
generator and no automatic seeding. It is available on the supported x86-64
Linux GNU target.

A generator's output is predictable from its seed, and from a few outputs. Do
not use it for keys, tokens, passwords, or anything an adversary must not
guess; use [`std.secrets`](secrets.md). Collection hashing is unrelated to both
modules.

```plenty
import std.random
from std.random import RandomError

def main() -> Result[(), AllocError | IoError | RandomError]:
    mut rng = std.random.seeded(2026u64)
    print(rng.between(1, 7)?, rng.between(1, 7)?, rng.next_bool())?
    mut deck = ["A", "B", "C", "D", "E"]?
    rng.shuffle(&mut deck)
    print(deck)?
    print(rng.sample_indices(10, 3)?)?
    mut replay = std.random.seeded(2026u64)
    print(replay.between(1, 7)?)?
    Ok(())
```
```output
6 5 True
["D", "A", "E", "C", "B"]
[9, 7, 1]
6
```

## Generators

| Function | Result | Contract |
|---|---|---|
| `seeded(seed: u64)` | `Rng` | Expand the seed into a full state; every seed is valid |
| `from_state(a: u64, b: u64, c: u64, d: u64)` | `Result[Rng, RandomError]` | Restore a state taken from `state()`; all zeros is `ZeroState` |

`Rng` has private fields, so these two functions are the only way to make one.
To seed from the operating system, pass
[`std.secrets.random_u64()`](secrets.md) to `seeded` and record the seed if the
run must be repeatable.

Methods that draw take `self: &mut Rng`.

| Method | Result | Contract |
|---|---|---|
| `next_u64()` | `u64` | One step; all 64 bits are uniform |
| `next_u32()` | `u32` | The high half of one step |
| `next_bool()` | `bool` | The top bit of one step |
| `next_f64()` | `f64` | A multiple of 2^-53 in `[0, 1)` |
| `below(bound: u64)` | `Result[u64, RandomError]` | Uniform in `[0, bound)`; zero is `EmptyRange` |
| `between(low: i64, high: i64)` | `Result[i64, RandomError]` | Uniform in `[low, high)`; `low >= high` is `EmptyRange` |
| `index(count: i64)` | `Result[i64, RandomError]` | Uniform index into `count` elements; zero is `EmptyRange`, negative `OutOfRange` |
| `shuffle[T](items: &mut list[T])` | `()` | Uniform permutation in place |
| `sample_indices(count: i64, amount: i64)` | `Result[list[i64], RandomError]` | `amount` distinct indices below `count`, in selection order |
| `fill(target: &mut Bytes, start: i64, stop: i64)` | `Result[(), RandomError]` | Overwrite `target[start:stop]` |
| `state()` | `(u64, u64, u64, u64)` | The current state words; takes `&Rng` |

Ranges are half-open and never clamp. `between` covers any ordered pair of
`i64` bounds, including `i64.MIN` to `i64.MAX`. `fill` follows the
[range rule](bytes.md#the-range-rule).

`below`, `between`, and `index` are unbiased: values of a step that would make
some results more likely are rejected and another step is drawn. A draw takes
fewer than two steps on average for every bound.

`shuffle` is Fisher-Yates. It exchanges elements with `swap`, so it works for
any element type and never copies or drops an element. To pick `k` of `n`
elements, call `sample_indices(n, k)` and index the collection.

A rejected request (`EmptyRange`, `OutOfRange`, or a failed allocation) does
not advance the generator. Lists of fewer than two elements and empty `fill`
ranges also consume no steps.

## Algorithm and stability

`Rng` is xoshiro256\*\* 1.0: 256 bits of state and a period of 2^256 - 1.
`seeded` fills the state with four successive SplitMix64 outputs, as the
algorithm's authors recommend.

The sequences are version-stable. For a given seed or state, every method
above returns the same values in every Plenty release, on every thread and
run. The derived methods are part of that contract:

| Method | Definition |
|---|---|
| `next_u32` | `next_u64() >> 32` |
| `next_bool` | Bit 63 of `next_u64()` |
| `next_f64` | `(next_u64() >> 11) * 2^-53` |
| `below(bound)` | Draw `next_u64()` until the value is at least `2^64 mod bound`; return it modulo `bound` |
| `between(low, high)` | `low + below(high - low)`, computed in `u64` |
| `shuffle` | For `last` from `len - 1` down to `1`: swap `last` with `below(last + 1)` |
| `sample_indices` | Over the list `0..count`, for `position` from `0` below `amount`: swap `position` with `position + below(count - position)`; return the first `amount` |
| `fill` | Each step supplies eight bytes, least significant first; a final partial group takes the low bytes of one more step |

A different algorithm would be a new constructor, not a change to `seeded`.

## Errors

```text
pub enum RandomError:
    EmptyRange
    OutOfRange
    ZeroState
    Allocation(AllocError)
```

| Variant | Meaning |
|---|---|
| `EmptyRange` | The requested range holds no value |
| `OutOfRange` | A negative count, `amount > count`, or a byte range outside the buffer |
| `ZeroState` | `from_state` was given the all-zero state, which never advances |
| `Allocation(cause)` | `sample_indices` could not allocate its list |

## Ownership and threads

A generator is an ordinary owned value of four `u64` words stored inline.
Drawing needs an exclusive loan, so two parts of a program cannot interleave
draws from one generator by accident; give each its own, seeded differently.
The module holds no global state, passes the ordinary worker-effect checks,
and a generator can be moved into a worker thread. There is no built-in way to
derive independent streams from one seed: use distinct seeds.

## Costs

| Operation | Allocates | Time |
|---|---|---|
| `seeded`, `from_state`, `state` | Nothing | Constant |
| `next_u64`, `next_u32`, `next_bool`, `next_f64`, `below`, `between`, `index` | Nothing | Constant; expected constant for the bounded draws |
| `shuffle` | Nothing | Linear, one bounded draw per element |
| `fill` | Nothing | Linear, one checked `Bytes.set` per byte |
| `sample_indices(count, amount)` | One list of `count` indices | Linear in `count` |

`sample_indices` is proportional to `count`, not `amount`. Throughput has not
been measured on an idle machine.

## Limits

- One algorithm. No jump or split functions for parallel streams.
- Normal, exponential and floating uniform distributions are provided by
  [`std.stat`](stat.md), using an explicitly supplied `Rng`. Weighted choice
  is not provided.
- No `f32` draws and no inclusive ranges.
- `sample_indices` returns indices; there is no element-sampling or
  reservoir form.

## Acknowledgements

- Uses the [xoshiro256\*\*](https://prng.di.unimi.it/) generator by David Blackman and Sebastiano Vigna, seeded with [SplitMix64](https://doi.org/10.1145/2714064.2660195) by Guy Steele, Doug Lea and Christine Flood.
