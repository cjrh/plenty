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
| Rust | 243 | 286 | | |
| C | 158 | 208 | | |

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

## What remains

- **The hash function.** Rust spends 98 instructions in `hash_one` for the
  3-byte key; Plenty spends about 95 inline for an inline key, 102 for a 3-byte
  literal and 139 for 18 bytes. The hash is not where the gap to Rust is. It is
  where the gap to C's 158 is closed only by a cheaper function, which is a
  DoS-resistance decision.
- **Heap keys.** A heap key's hit runs four runtime functions
  (`get_or_insert_text`, `hash_text`, `probe_text`, `try_settle`) and, when the
  stored key is another object, `probe_text_bytes` with `memcmp`: about 100
  instructions of frames, spills and argument moves that the inline-key path no
  longer has. Inlining them into one function was measured for inline keys
  only.
- **Caller context around `to_text_range`.** `plenty_caller_enter` and
  `plenty_caller_leave` cost 21 instructions per key. The call can trap (a
  range outside the buffer), and its record names the user call only through
  that context, so it cannot be dropped while `to_text_range` is a call.
  Moving it to the cold path needs the range check inlined into the caller
  with its site named there, which needs two compiler changes: inlining a
  body that ends in a tail call, and a transitive "reads no caller context"
  property so the conversion can stay a plain call. Not done.
- **Table layout.** One 64-bit bucket per slot at load 1/2 with linear probing
  already answers these hits at the first bucket; grouped probing would not
  reduce them.

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
