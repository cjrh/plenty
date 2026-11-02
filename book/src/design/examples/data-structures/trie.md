# Retrieve strings with a trie

A **trie**, or retrieval tree, shares common prefixes between keys. Each edge
here represents one Unicode scalar. The path `c → a → r` represents `"car"`;
a terminal marker distinguishes that stored key from a prefix that merely leads
to `"cart"`. The permanent root represents the empty string.

This example implements a set of strings directly in Plenty, including exact
lookup, insertion, pruning deletion, prefix enumeration, and longest-prefix
matching. It uses private node IDs rather than stored references. It is an
educational implementation, not a new `std` API.

## Run the example

From a repository checkout:

```sh
plenty book/src/design/examples/data-structures/trie/main.plenty
plenty book/src/design/examples/data-structures/trie/checks.plenty
```

Or save the [implementation](trie/trie.plenty), [demo](trie/main.plenty), and
[test driver](trie/checks.plenty) in the same directory, then run
`plenty main.plenty` or `plenty checks.plenty`. These are the canonical source
files compiled by the Rust regression tests. The demo below includes its source
rather than maintaining another copy:

```plenty fragment
{{#include trie/main.plenty}}
```

Expected output (the visually similar café spellings are distinct keys):

```text
["", "café", "café", "car", "cart", "cat", "dog", "猫", "猫咪"]
True False
True False
["car", "cart"]
["猫", "猫咪"]
Option[str].Some("cart")
Option[str].Some("")
False
True True
["cart"]
True
Option[str].Nothing
True
```

## Interface

| Operation | Result | Meaning |
| --- | --- | --- |
| `create()` | `Result[Trie, AllocError]` | Create an empty set with a permanent root. |
| `length(&tree)` | `i64` | Count stored keys, including the empty string if present. |
| `insert(&mut tree, key)` | `Result[bool, AllocError]` | `True` for a new key; duplicates return `False`. |
| `has(&tree, key)` | `bool` | Test for a complete stored key, not just a path. |
| `remove(&mut tree, key)` | `bool` | Delete a key and prune unused suffix nodes; missing keys return `False`. |
| `has_prefix(&tree, prefix)` | `bool` | Whether any stored key starts with the prefix. |
| `with_prefix(&tree, prefix)` | `Result[list[str], AllocError]` | All matching keys in scalar-lexicographic order, including the prefix itself if stored. |
| `ordered(&tree)` | `Result[list[str], AllocError]` | All keys; equivalent to `with_prefix(&tree, "")`. |
| `longest_prefix(&tree, text)` | `Option[str]` | The longest **stored key** that prefixes the text. |
| `valid(&tree)` | `Result[bool, AllocError]` | Check structure, terminals, pruning, and free-list membership. |

All key, prefix, and text parameters borrow `&str`; the implementation keeps
immutable string handles only when needed. Exact lookup, prefix existence,
longest-prefix matching, and deletion allocate nothing. Insertion can allocate
node storage; enumeration allocates its result list; validation allocates
visitation marks and a worklist.

For an empty set, `has_prefix(&tree, "")` is `False`, enumeration returns an
empty list, and `longest_prefix` returns `Nothing`. If `""` is stored,
longest-prefix matching returns at least `Some("")`, even when no nonempty key
matches. Removing a key does not remove its longer descendants or shorter
stored prefixes.

## Unicode and ownership

Matching is exact and case-sensitive. There is no normalization, case folding,
locale collation, or grapheme-cluster segmentation. `"é"` and `"e\u{301}"`
follow different paths. A combining mark is its own edge; an emoji outside the
basic multilingual plane is one scalar, not several byte edges. Embedded NUL
and the maximum Unicode scalar are accepted like other valid string content.

Terminal nodes retain full keys as immutable `str` handles, so enumeration
need not rebuild each string from its path. Returned strings remain valid after
deletion, slot reuse, or dropping the trie. They share immutable storage rather
than copying key bytes; an inserted heap-backed key can therefore keep that
storage alive. See [text storage and iteration](../../facilities/01-builtins/text.md).

## Validation

The [model driver](trie/checks.plenty) compares the trie with a sorted-list set
and independent prefix/longest-prefix scans. It checks 14,400 pairs of five-key
insertion/deletion permutations, duplicates, missing removals, complete pruning
and refill, plus 8,000 mixed operations over Unicode, case-sensitive, empty,
NUL-containing, and prefix-related keys. It also checks a 10,000-scalar key and
returned strings surviving deletion and owner destruction.

The integration tests additionally sweep insertion and prefix-result allocation
failures, retry failed insertion, disable allocation during lookup/pruning/reuse,
and exercise a 100,000-scalar key under a 256 KiB native stack on Linux:

```sh
cargo test --locked --features runtime-checks --test integration test_data_structure_patterns::trie
```

Read [trie storage, updates, and costs](trie-internals.md) for the implementation
and its limits. Compare the [balanced search trees](balanced-trees.md) when
ordering alone, rather than shared-prefix retrieval, is the main requirement.
