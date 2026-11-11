# Dictionary update hits (issues #931 and #949)

`*counts.get_or_insert(key, 0)? += 1` on a key that is present is the
k-nucleotide counting step. This directory isolates it.

## Kernels

- [`hits-K-N.plenty`](hits-3-1000000.plenty): N updates of one literal key of K
  bytes. A literal is heap-format text at any length.
- [`built-K-N.plenty`](built-3-1000000.plenty): the key is built per pass with
  `Bytes.to_text_range`, as k-nucleotide does; a 3-byte key is inline text and
  an 18-byte key is allocated and released per pass.
- [`borrowed-K-N.plenty`](borrowed-3-1000000.plenty): the same update with
  `get_or_insert_bytes(&sequence[a:b], 0)`, which reads the key's bytes in
  place (issue #949); see [borrowed keys](#borrowed-keys-issue-949).
- [`misses-built-K-N.plenty`](misses-built-7-200000.plenty) and
  [`misses-borrowed-K-N.plenty`](misses-borrowed-7-200000.plenty): N updates
  that each insert a new K-byte key, in both forms.
- [`borrowed.rs`](borrowed.rs): owned keys looked up by a borrowed byte range,
  as `HashMap<String, i64>` by a checked `&str` and as `HashMap<Vec<u8>, i64>`
  by `&[u8]`.
- [`hits.rs`](hits.rs): `HashMap<&str, i64>` with `entry(key).or_insert(0)`
  (SipHash-1-3, SwissTable). [`hits.c`](hits.c): SipHash-1-3 and one linear
  probe over a word table.

Instructions per pass are the difference between the 2,000,000 and 1,000,000
pass programs, `perf stat -e instructions:u`, release compiler, Ryzen 7 7840HS.

| Step | hits k=3 | hits k=18 | built k=3 | built k=18 |
|---|---:|---:|---:|---:|
| Before | 454 | 491 | 528 | 843 |
| Operands and result in registers (`plenty_dict_get_or_insert`) | 441 | 477 | 513 | 829 |
| Text probe by word, byte comparison out of the loop | 379 | 415 | 438 | 767 |
| Text hash without the streaming hasher | 371 | 407 | 410 | 759 |
| Hit without a `Slot`, division-free row check, hash dispatch in the caller | 335 | 372 | 371 | 724 |
| `plenty_dict_get_or_insert_text` | 333 | 371 | 317 | 724 |
| Heap-key hit in one function | 254 | 288 | 310 | 640 |
| Key word and value slot from one row check | 226 | 262 | 293 | 615 |
| Rust | 243 | 286 | | |
| C | 158 | 208 | | |

Time for the last two steps, `perf stat -r 10` to `-r 15` pinned to one core
on a quiet machine (load average 1.3 to 3), cycles and nanoseconds per pass:

| Program | hits k=3 | hits k=18 | built k=3 | built k=18 |
|---|---:|---:|---:|---:|
| Before the heap-key steps | 67.7 cycles, 14.5 ns | 77.7, 16.6 | 74.4, 16.0 | 146.9, 31.5 |
| Heap-key hit in one function | 54.2, 11.6 | 62.3, 13.4 | 65.0, 13.9 | 132.4, 28.5 |
| One row check | 51.7, 11.3 | 59.0, 12.7 | 62.9, 13.8 | 137.1, 29.6 |
| Rust | 57.2, 12.1 | 63.6, 13.6 | | |
| C | 80.0, 17.0 | 84.8, 18.0 | | |

Readings of one program vary by about 3% between runs: in two interleaved
`-r 15` rounds of the 2,000,000-pass programs the final code took 22.2 to
22.6 ms against 30.2 to 32.2 (hits k=3), 26.0 to 26.4 against 33.0 to 34.2
(hits k=18), 27.7 to 27.9 against 29.2 to 29.5 (built k=3) and 58.2 against
65.3 to 66.0 (built k=18). The C kernel runs the fewest instructions and takes
the longest; the likely cause, not verified, is its tail `memcpy` of a variable
length into the final block, a store the following load cannot forward from.
Instruction counts rank these programs wrongly; time is the measure.

`hits.rs` and `hits.c` correspond to the `hits` kernels only. The `built`
columns include key construction (about 105 instructions for an inline key;
see [`../key-construction/`](../key-construction/README.md)).

## Borrowed keys (issue #949)

`get_or_insert` takes an owned key, so the `built` kernels allocate, validate,
copy and release an 18-byte key on every hit. `get_or_insert_bytes` hashes and
compares the bytes in the buffer and builds the key on a miss only. Per pass,
main at 1d091967 against the branch, same machine, load average 2 to 3.5;
cycles and time are two interleaved rounds of `perf stat -r 12` pinned to one
core:

| Program | Instructions | Cycles | ns |
|---|---:|---:|---:|
| `built` k=3, main | 292 | 62.3 to 63.0 | 13.4 to 13.6 |
| `built` k=3, branch | 293 | 63.0 to 63.2 | 13.7 |
| `borrowed` k=3 | 204 | 44.0 to 48.9 | 9.7 to 10.6 |
| Rust `String` by checked `&str`, k=3 | 316 | 69.6 to 70.8 | 15.0 to 15.6 |
| Rust `Vec<u8>` by `&[u8]`, k=3 | 260 | 59.4 to 59.5 | 12.8 |
| `built` k=18, main | 613 | 131.4 to 131.7 | 28.3 to 28.6 |
| `built` k=18, branch | 613 | 129.3 to 133.4 | 27.9 to 28.8 |
| `borrowed` k=18 | 247 | 53.8 to 54.4 | 11.6 to 11.7 |
| Rust `String` by checked `&str`, k=18 | 470 | 96.7 to 97.0 | 20.9 to 21.3 |
| Rust `Vec<u8>` by `&[u8]`, k=18 | 304 | 76.7 to 82.8 | 16.9 to 17.9 |

In the same rounds the `hits` kernels (a literal key, no construction) took
226 and 262 instructions, 49.7 to 50.1 and 58.8 to 58.9 cycles; Rust's
`hits.rs` 56.3 to 57.1 and 64.1 to 74.1 cycles; C 79.6 and 85.2.

The borrowed update runs fewer instructions and cycles than either Rust form.
Safe Rust validates the range before every lookup (`from_utf8`); here a hit
validates nothing, because bytes equal to a stored key are valid UTF-8. The
C kernel runs 158 and 208 instructions. The 46 and 39 more here are, read
from the disassembly (approximate):

| Where | Instructions | Cause |
|---|---:|---|
| `main` | 8 | `plenty_byte_view` reads the buffer's address and length on every pass; the call is not hoisted out of the loop |
| `main` | 4 | The range check of `&sequence[1:1 + K]` and the view's address |
| `main` | 4 | Three helper arguments reloaded from stack slots and one moved |
| `main` | 4 | The hit flag is tested and cleared, then the `Result` tag tested |
| `main` | 3 | The count is loaded, added with an overflow branch and stored |
| Helper | 14 | Six callee-saved registers and a frame, pushed and popped on the hit although only the tail-call paths need them |
| Helper | 8 | Empty-table and hash-key-initialized checks; the address of an empty view replaced by a non-null one |

The helper's hash, probe and comparison otherwise match the C function's
count (153 at k=3) to within a few instructions: the row bounds check and the
length dispatch of the final block here against C's `memcpy` and `memcmp`
calls.

Self instructions per pass (callgrind): `plenty_dict_get_or_insert_bytes` 175
(k=3) and 218 (k=18), `main` 23, `plenty_byte_view` 6.

Misses, each pass inserting a new key (difference between the 400,000 and
200,000 pass programs; two rounds):

| Program | Instructions | Cycles |
|---|---:|---:|
| `misses-built` k=7, main | 1137 | 441 to 593 |
| `misses-built` k=7, branch | 1137 | 429 to 473 |
| `misses-borrowed` k=7 | 1083 | 424 to 466 |
| `misses-built` k=18, main | 1438 | 579 to 609 |
| `misses-built` k=18, branch | 1438 | 601 to 611 |
| `misses-borrowed` k=18 | 1340 | 557 to 652 |

The owned-key insert is unchanged. A borrowed miss runs fewer instructions
(it hashes once and inserts at the probed bucket); its time is within the
run-to-run spread, which growth and page faults dominate.

k-nucleotide at fasta 25,000,000, three runs per round, two interleaved
rounds, output identical:

| Program | Instructions | Cycles | Time |
|---|---:|---:|---:|
| Serial, main | 355.8B | 118.8B to 124.6B | 25.7 to 27.4 s |
| Serial, branch | 204.0B to 204.2B | 68.9B to 69.6B | 15.1 to 15.4 s |
| Parallel (7 workers), main | 357.8B to 358.1B | 129.6B to 130.8B | 9.40 to 9.57 s |
| Parallel (7 workers), branch | 205.7B to 205.9B | 81.3B to 82.1B | 5.43 to 5.62 s |

The Rust reference (7 threads) took 12.3 s, 329.5B instructions and 177.7B
cycles in the same session.

Memory now dominates. In the serial program 91.8% of cycle samples are in
`plenty_dict_get_or_insert_bytes`, and 44% of that function's samples are on
the instruction after the first bucket load; the stored key word's load has
5% and the heap key comparison 2.5%. Cache misses are 256M per run on main
and on the branch: the change removed key construction, not misses.

## Attribution

| Cause | Instructions removed | Fix |
|---|---:|---|
| The dispatcher wrote the operands to an argument block, checked the descriptor and unpacked the result | 13-15 | Codegen calls a helper with the dictionary, key and value in registers; the two result words return in registers |
| Each probe candidate dispatched on the key's storage, read a 16-byte key and called the general `equal` | 62-75 | A text probe reads candidate words and compares words; `strings::equal` runs out of line only when they differ |
| `SipHasher13` buffered the bytes through its streaming state; short inline keys went through the length-dependent copy | 8-28 | Blocks are read from the slice; an inline text is its final block already |
| `locate` returned a seven-word `Slot` that only a miss reads; the row check divided the data length by the stride; `hash` framed the encoding of all key kinds | 36-39 | The hit settles from the bucket; a miss builds the `Slot` out of line; the row check multiplies; the kind dispatch is in the caller |
| The inline-key hit called `hash_text` and `probe_text` and kept a frame across them, and dispatched twice on the key kind | 54 (built k=3) | A helper for `str` keys selects by the key's form with a tail call; the inline-key hit hashes and probes with no call |
| The heap-key hit ran `get_or_insert_text`, `hash_text` with `sip13_bytes`, `probe_text`, `try_settle` and, for a stored key in another object, `probe_text_bytes` with `memcmp`: four or five frames and their argument moves | 79-84 | `get_or_insert_heap_text` hashes, probes and compares inline: whole words and one overlapping last word for eight bytes or more, a byte loop below that. Every other outcome leaves through a tail call |
| A hit checked the row twice (key word, then value slot), read the bucket again to find the entry, and computed the value's offset from the key descriptor | 17-28 | `Entries::text_row` returns the key word and the value slot from one row check; a text key is one slot word |

Self instructions per pass after the changes (callgrind):

| built k=3 | | hits k=18 | |
|---|---:|---|---:|
| `get_or_insert_inline_text` (hash about 95) | 166 | `sip13_bytes` + `hash_text` | 139 |
| `Bytes.to_text_range` | 47 | `get_or_insert_text` | 64 |
| `main` | 43 | `probe_text` | 38 |
| `plenty_bytes_to_text` | 21 | `probe_text_bytes` + `memcmp` | 59 |
| `plenty_caller_enter` + `plenty_caller_leave` | 21 | `try_settle` | 38 |
| `plenty_release` | 9 | `main` | 25 |
| `plenty_byte_view` + `plenty_bytes_len` | 8 | `plenty_release` | 12 |

After the heap-key steps, hits k=18 is `get_or_insert_heap_text` 222, `main`
25, `plenty_release` 12 and the dispatching helper 3.

## What remains

- **The hash function.** SipHash-1-3 stays (decided for DoS resistance). It
  is most of the hit: about 95 instructions for an inline key and about 130
  for 18 bytes, of 166 and 222 in the hit function.
- **Heap keys.** The hit is one function, `get_or_insert_heap_text`: 222
  instructions for an 18-byte key whose stored key is another object. It
  saves six registers and keeps three values on the stack across the hash,
  which uses every caller-saved register.
- **Releasing a literal key.** A literal key's hit still calls
  `plenty_release` on the immortal literal (12 instructions).
- **Caller context around `to_text_range`.** 21 instructions per key; issue
  #948. A key looked up with `get_or_insert_bytes` is not built on a hit.
- **Memory.** With an owned key, k-nucleotide's heap-key hit spent its time in
  two loads: the bucket (18% of the function's samples) and the first key
  block (18%). With a borrowed key the bucket load alone has 44% of the hit
  function's samples. The kernels here keep three keys in cache and do not
  show this.
- **The byte view per pass.** `&sequence[a:b]` in a loop calls
  `plenty_byte_view` on every pass (9 instructions with the call) although the
  buffer is borrowed for the whole loop.
- **Table layout.** One 64-bit bucket per slot at load 1/2 with linear probing
  answers these kernels' hits at the first bucket. Probe lengths and cache
  misses in k-nucleotide's larger dictionaries were not measured against
  another layout.

## Rejected

- Replacing the division in the existing row assertion in place: the new
  check was not shared with `key_word`, and the hit kept both.
- One function for the whole text probe including byte comparison: the call
  to `strings::equal` put a frame on the word-equal path.
- A slow path that loops back into the probe: the loop kept the candidate
  load's bounds checks in the hot block.
- The inline-key fast path inside `plenty_dict_get_or_insert`: its frame and
  the nested general call cost every other key 54 instructions (388 / 426 /
  346 / 778). Selecting in codegen by key type, with tail calls in the helper,
  removed that cost.
