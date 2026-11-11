# Dictionary update hits (issue #931)

`*counts.get_or_insert(key, 0)? += 1` on a key that is present is the
k-nucleotide counting step. This directory isolates it.

## Kernels

- [`hits-K-N.plenty`](hits-3-1000000.plenty): N updates of one literal key of K
  bytes. A literal is heap-format text at any length.
- [`built-K-N.plenty`](built-3-1000000.plenty): the key is built per pass with
  `Bytes.to_text_range`, as k-nucleotide does; a 3-byte key is inline text and
  an 18-byte key is allocated and released per pass.
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

The Rust and C programs correspond to the `hits` kernels only. The `built`
columns include key construction (about 105 instructions for an inline key;
see [`../key-construction/`](../key-construction/README.md)).

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
  #948.
- **Memory.** In k-nucleotide the heap-key hit's time is in two loads: the
  bucket (18% of the function's samples) and the first key block (18%). The
  kernels here keep three keys in cache and do not show this.
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
