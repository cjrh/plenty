# Draw random values and make secrets

Plenty has two sources of random values, and they are not interchangeable.
`std.random` gives repeatable sequences for simulations, games, and tests.
`std.secrets` gives unpredictable bytes from the operating system for tokens
and keys.

For record and request identifiers, the next lesson introduces
[`std.uuid`](20-standard-uuid.md), which builds standard UUIDs from OS entropy.

## Seeded generators

A generator is a value you create from a seed and own. Nothing is seeded for
you and there is no shared generator behind the scenes, so the same seed always
produces the same run:

```plenty
import std.random

def main() -> Result[(), AllocError | IoError | std.random.RandomError]:
    mut rng = std.random.seeded(1234567u64)
    print(rng.next_u64())?
    print(rng.between(1, 7)?, rng.between(1, 7)?, rng.between(1, 7)?)?
    print(rng.next_bool(), rng.next_f64() < 1.0)?
    mut cards = ["ace", "king", "queen", "jack", "ten"]?
    rng.shuffle(&mut cards)
    print(cards)?
    print(rng.sample_indices(10, 3)?)?
    print(cards[rng.index(len(cards))?])?
    Ok(())
```
```output
3504822795582309479
5 2 1
True True
["ace", "king", "ten", "queen", "jack"]
[3, 1, 9]
king
```

Drawing changes the generator, so it must be a `mut` binding. Ranges are
half-open: `between(1, 7)` is a die roll from 1 to 6. Every bounded draw is
unbiased. `shuffle` reorders a list in place without copying its elements.
`sample_indices(10, 3)` picks three different positions out of ten; index your
collection with them. `index(count)` picks one.

Because the sequence depends only on the seed, a failing simulation can be
replayed exactly. `state()` and `from_state` checkpoint a generator part-way
through:

```plenty
import std.random

def main() -> Result[(), AllocError | IoError | std.random.RandomError]:
    mut first = std.random.seeded(42u64)
    mut second = std.random.seeded(42u64)
    print(first.next_u64() == second.next_u64())?
    (a, b, c, d) = first.state()
    mut resumed = std.random.from_state(a, b, c, d)?
    print(first.between(0, 100)? == resumed.between(0, 100)?)?
    print(str.repr(first.between(5, 5))?)?
    print(str.repr(first.below(0u64))?)?
    print(str.repr(first.sample_indices(3, 4))?)?
    Ok(())
```
```output
True
True
Result[i64, std.random.RandomError].Err(std.random.RandomError.EmptyRange)
Result[u64, std.random.RandomError].Err(std.random.RandomError.EmptyRange)
Result[list[i64], std.random.RandomError].Err(std.random.RandomError.OutOfRange)
```

An empty range is an error, never a clamped or default value. A rejected
request does not advance the generator.

The sequence for a seed is part of the language's contract and does not change
between Plenty versions. It is also predictable by anyone who sees a few
outputs, so never use a seeded generator for a password, token, or key.

## Secrets

`std.secrets` asks the operating system for every byte. Its results differ on
every run, so this program prints sizes rather than values:

```plenty
import std.bytes
import std.secrets

def main() -> Result[(), AllocError | IoError | std.bytes.BytesError | std.secrets.SecretsError]:
    token = std.secrets.token_urlsafe(32)?
    print(len(token))?
    print(len(std.secrets.token_hex(16)?))?
    key = std.secrets.token_bytes(32)?
    print(key.len())?
    print(token != std.secrets.token_urlsafe(32)?)?
    print(std.secrets.below(6u64)? < 6u64)?
    mut nonce = std.bytes.empty()?
    nonce.resize(12, 0u8)?
    std.secrets.fill(&mut nonce, 0, 12)?
    print(nonce.len())?
    print(str.repr(std.secrets.token_hex(-1))?)?
    Ok(())
```
```output
43
32
32
True
True
12
Result[str, std.secrets.SecretsError].Err(std.secrets.SecretsError.OutOfRange)
```

The argument is a number of secret bytes, not characters: 32 bytes become 43
base64url characters or 64 hex digits. `token_urlsafe` is safe to put in a URL
or a file name. `below` is an unbiased integer, for example to pick a winner.

If the system cannot supply entropy, these functions return
`SecretsError.Entropy`. They never fall back to a weaker source, and a failed
`fill` leaves zeros, not a partly random buffer.

To start a simulation from an unpredictable seed, pass
`std.secrets.random_u64()?` to `std.random.seeded` and print the seed so the run
can be repeated.

For normal, exponential or floating uniform samples, pass that generator to
a [`std.stat` distribution](22-statistics.md). Each distribution documents its
draw count, and failed sampling leaves the generator unchanged.

See the [random reference](../../design/stdlib/random.md) for the algorithm and
its stability contract, and the [secrets reference](../../design/stdlib/secrets.md)
for the failure policy and costs.
