# Key construction from a byte range (issue #931)

`sequence.to_text_range(start, start + k)?` builds each k-nucleotide key. At
fasta 50,000 (1,749,961 keys) the chain cost 173 instructions per key before
the dictionary was touched.

## Kernel

[`keys-K-N.plenty`](keys-3-1000000.plenty) builds one key per pass and adds its
byte length; [`keys.rs`](keys.rs) does the same with
`String::from_utf8(bytes[start..stop].to_vec())` and [`keys.c`](keys.c) is the
floor: range check, ASCII test, packed word or heap copy. Instructions per
pass are the difference between the 2,000,000 and 1,000,000 pass programs,
`perf stat -e instructions:u`, release compiler, Ryzen 7 7840HS.

| Kernel | C | Rust | Plenty before | Plenty after |
|---|---:|---:|---:|---:|
| 3-byte key (inline text) | 87 | 155 | 230 | 151 |
| 12-byte key (heap text) | 196 | 293 | 440 | 397 |

A pass includes the loop, the caller-site bookkeeping around the std call
(`plenty_caller_enter`, `plenty_caller_leave`) and the key's release.

## Attribution

Self instructions per key in k-nucleotide (callgrind, fasta 50,000):

| Function | Before | After |
|---|---:|---:|
| `Bytes.to_text_range` | 54.0 | 36.0 |
| `std.bytes.text_in` | 28.0 | 18.0 |
| `plenty_op_183` / `plenty_bytes_to_text` | 25.0 | 20.6 |
| `strings::try_from_utf8` | 53.4 | 22.0 |
| `plenty_byte_view_len` + `plenty_byte_view_rows` / `plenty_byte_view` | 11.0 | 6.0 |
| `plenty_bytes_len` | 2.0 | 2.0 |
| Total | 173.4 | 104.6 |

| Cause | Fix |
|---|---|
| The slice `&self[start:stop]` called one helper for the buffer's length and another for its address | One `plenty_byte_view` call returns both in the two return registers |
| The slice clamped both bounds (sign tests, `len` additions, two selects each, a third clamp of `stop` against `start`) after `within` had validated them | Two unsigned compares take the slice as written when `start <= stop <= len`; any other operands run the same clamps in a cold block |
| `__bytes_to_text` went through the dispatcher: six argument-block stores, descriptor and site arguments, two result loads, and `plenty_op_183`'s operation decode | `plenty_bytes_to_text(address, count)` takes the view in registers and returns the result words in registers |
| `strings::pack` copied a short key one byte per loop pass, inside `try_from_utf8`'s six-register frame | `pack` reads at most two overlapping fixed-width loads; `plenty_bytes_to_text` answers an ASCII key of at most 7 bytes inline and calls `try_from_utf8` only for longer or non-ASCII ranges |

## Variants

| Variant | 3-byte key | 12-byte key |
|---|---:|---:|
| Baseline | 230 | 440 |
| One view helper, in-range slice path, narrow conversion helper, load-based `pack` | 182 | 397 |
| Plus the inline ASCII answer in `plenty_bytes_to_text` | 151 | 397 |

Rejected: proving the slice bounds from `within` (codegen keeps no facts from
branch conditions, and the conditions of the inlined call are already
jump-threaded); an unchecked slice intrinsic (a new trusted operation); reading
the buffer's `Vec` fields inline (their offsets are not a stable layout).

A separate attempt on the dictionary side replaced the division in
`Entries`' row bound (`index < data.len() / stride`, in `key`, `get` and
`slot_address`) with a checked multiplication. k-nucleotide rose from 1.282B to
1.297B instructions, so it was not kept.

## Whole program

| Program | Before | After |
|---|---:|---:|
| k-nucleotide, fasta 50,000 (`perf stat`, one core) | 1,403,352,781 | 1,282,239,004 |
| k-nucleotide idiomatic, `bench.py` quick | 6,330,200,581 | 5,722,688,477 |
| k-nucleotide parallel, `bench.py` quick | 6,330,884,429 | 5,727,018,404 |
| fasta, `bench.py` quick | 1,521,463,860 | 1,501,036,081 |
| regex-redux, `bench.py` quick | 560,963,876 | 558,809,252 |

Outputs are identical. The other `benchmarks/game` programs move by less than
0.001%. Compiling `knucleotide.plenty` takes 419,784,373 instructions against
419,584,672 (+0.05%).

## Remaining in this chain

After the change a 3-byte key still spends 64 instructions more than C:
caller-site bookkeeping (21 in the two helpers), the second range test after
`within`, the separate `plenty_bytes_len` call, and re-encoding the
`Result[Option[str], AllocError]` words as `Result[str, BytesError]`.
