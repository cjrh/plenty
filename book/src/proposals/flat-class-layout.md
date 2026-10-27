# Flat class layout

> **Status: Stage 1 (one-field classes as newtypes) and stage 2 (flat nested
> class records) implemented.** Issue #547 asked why a class
> value is an address into its own storage, and whether classes could instead
> be laid out flat inside their owner. [Values](../design/toolchain/02-architecture/values.md)
> and [memory](../design/toolchain/02-architecture/memory.md) own the shipped
> contract; this page records the findings and the design.

## Why a class value holds an address

Every runtime slot is 128 bits: a payload word and a tag word. A class keeps
its fields in *inline storage* that follows its slot in the owner (a local, a
record field, a list element, a box), and the payload word is the address of
that storage. The address exists because the payload word has to be *something*
uniform:

- Runtime operations (`copy_payload`, `release`, `retain`, hashing, equality,
  formatting) receive one `u128` and a type descriptor and walk the record
  through the descriptor. They never receive the slot address.
- References are `(address, shift)` pairs. `&self.field` is computed from the
  address of the record, which is read out of the class value word; a
  reference to a class value therefore projects fields with one load and an
  add, not by knowing where the value's slot lives.
- Sum payloads are packed into the same 128 bits as their tag; a variant
  holding a class holds its address word and leaves the tags free.
- A zero payload word means "empty slot" for every type, which lets moved-out
  locals, fields, and partially initialized records release for free.

The cost is that every move of a class value is a copy of its inline bytes
plus a *fix* of each nested class, range, closure, or inline-sum address
(`inline.rs::fix_record`, the runtime's `Payload::relocate`); issue #544
measured that as the remaining overhead of nested wrappers.

### What depended on the address before stage 2

| Dependent | How it uses the word |
| --- | --- |
| `lower_class` (`src/codegen/enums.rs`) | `Field` loads a slot at `word + offset`; `FieldRef` loads the word from the reference's address and adds the offset |
| `copy_inline_to`, `copy_record`, `fix_*`, `release_record` (`src/codegen/inline.rs`) | memcpy from the word, then rewrite nested words at `base + offset + 16` |
| runtime `Type::record` (`plenty-runtime/src/aggregates.rs`) | `value as u64 as *mut u128` is the record base for walks, drops, copies, hashing, equality, formatting |
| references (`src/codegen/references.rs`) | `write_reference` copies inline payloads to `address + 16` and stores the word at `address` |
| sum payloads (`src/codegen/enums.rs`, runtime `Type::payload`) | a variant's single class field is the class's 128 bits; tags stack in the high word |
| generators and closures | class captures and frame slots are ordinary slots with following inline bytes |
| `Box[T]`, `Shared[T]`, actors | the box holds one slot plus inline bytes; copies between actors use `copy_payload` |
| C export (`src/codegen/exports.rs`, `src/exports/handles.rs`) | a handle is a `Box` holding the instance; a borrowed `&Class` is the slot address inside the handle; an owned argument is copied out with `copy_payload` using the class descriptor |
| columnar lists (`plenty-runtime/src/aggregates/clist.rs`) | the row class descriptor's variants are the column schema |

## Stage 1: one-field classes are newtypes

**Rule.** A class with exactly one field and no `__del__` is represented
exactly as that field: same 128-bit slot, same inline bytes, same descriptor
layout. Constructing it, reading or borrowing its field, and moving the field
out are all identity operations in the generated code. No annotation; the rule
is total.

Classes with `__del__` are excluded because their record carries a state word
(initialization completed, observer count) that has no home in the field's
own slot. Field-less classes already lower to a zero word and keep doing so.

### Compiler

- `ClassType::newtype_field` (`src/record.rs`) names the rule once;
  `Ty::newtype_field` and `Ty::newtype_base` expose it.
- Layout queries delegate to the field: `has_inline_storage`, `flat_record`,
  `inline_bytes`, and the generator frame measurement.
- `lower_class`: `New` is a zero word (an empty slot, which the field's
  `__init__` write fills); `ArmDrop` is a no-op; `Field` and `FieldRef`
  return their operand unchanged. The final `unpack` to the field's native
  type is the only code emitted.
- `retain`, `release`, and `copy_inline_to` unpack to the field type and
  recurse. `fix_slot`, `fix_variant`, `slot_fixes`, `plain_variants` and
  `release_fields` look through newtypes, so a record or sum holding a newtype
  copies and releases as if it held the field directly.
- Columnar-list row access reads a one-field row as its scalar.
- A newtype has no inline storage, so a function returning one built by a
  constructor call ends in a tail call. The constructor inlining from
  issue #548 now also lowers at that site: the caller's locals are released
  first, as a real tail call would, and the shared exit then finds empty slots.

### Runtime descriptor

A newtype's descriptor carries the *field's* layout (kind, affinity, inline
bytes, links, variants, destructor hook) so every runtime walk treats the
value as the field. The class name and a new `wrapper` link (byte 64; the
descriptor grows from 64 to 72 bytes) record the source-level shape for
formatting, which prints `Wrap(x=1)`, and for the columnar-list schema.

### FFI layout contract

C code never sees a class layout: a class crosses the boundary as an opaque
handle (a `Box` holding one slot plus the descriptor's inline bytes). The rule
changes only what that slot holds for one-field classes, and every handle
operation goes through the descriptor, so exported libraries, foreign pointers,
and dynamic exports keep working without a header change. The contract FFI
relies on is therefore:

1. A handle points at a `Box` whose value slot is at offset 32.
2. A borrowed class argument is the address of that slot.
3. An owned class argument or result is copied in or out with the class
   descriptor's `copy_payload`.

These three hold for every class, newtype or not.

### Costs and limits

- `Type` grows by 8 bytes, so every program's read-only data changes; machine
  code for programs without one-field classes is unchanged.
- A one-field class with `__del__` keeps the record layout.
- Formatting a newtype walks one extra descriptor link.

### Measurements

Retired instructions (`perf stat -e instructions:u`) against main at c78365ad,
which includes constructor inlining, compiled with `--release`:

| Program | Before | After |
|---|---|---|
| Issue #544 reproduction, per `cycle` (file wrapped twice, 2000 iterations) | 486 | 444 |
| Issue #544 reproduction, 200 iterations, whole program | 340,245 | 331,699 |
| 200 loopback `connect`/`accept`/`close` pairs through `std.net` | 2,194,678 | 2,082,256 |
| Nested two-field records copied through a loop (no one-field class) | 283,245 | 283,175 |

The remaining per-cycle cost is caller-site bookkeeping around releases of
slots the compiler could prove empty, not record copies. Programs without a
one-field class produce identical machine code; only read-only data differs.

## Stage 2: flat nested records

**Goal.** Store a class-typed field at a fixed offset inside the outermost
owner without a word of its own, so a move is a byte copy with no address
fixes.

**Findings.** After `__init__`, a nested class field's word is redundant: it
is always `slot + 16` or zero (moved out). Dropping the word ("rule B") saves
16 bytes and one load, test and store per nested class per move. It needs:

- a presence marker for nested classes with `__del__` (their state word can
  serve; `FieldMove` must clear it) and for partially moved records;
- `record_field`, `fix_*`, `release_record`, and the runtime `Record::slots`
  walkers to compute nested addresses from the owner instead of loading them;
- references to nested fields to stay `(address, shift)` pairs, which already
  works because `FieldRef` adds offsets to an address.

What it does not remove: the top-level word stays an address, because runtime
operations and sum payloads still receive one `u128`. Flattening below the top
level removes fixes but not copies; a move still copies the inline bytes.

**First assessment.** Stage 2 was first deferred: after stage 1 the remaining
per-move cost of a nested record looked bounded by `RECORD_FIX_LIMIT`. Measuring
it showed otherwise: in a loop that moves a three-level record through a
function, the slot fixes were about 90 of the 146 instructions the callee
compiled per call (6 fixes of 5 instructions per record copy, 3 copies), and
the nested words added 96 of the 272 bytes copied per record.

**Decision.** Adopted. A field of a class record whose type is a class with a
record is stored flat. The cases that keep a word followed by a record are:

- top-level class values, whose word is the uniform 128-bit value runtime
  operations and sum payloads receive;
- class payloads of enum variants and tuple components: only class records
  flatten their fields, so variant payload packing and tuple layout are
  unchanged;
- field-less classes without `__del__`, which have no bytes to place.

### Contract

- **Presence.** An all-zero flat record is an empty field. Moving a field out
  or releasing it zeroes its bytes; a class with `__del__` has a nonzero state
  word once constructed, and every other record with a record has at least one
  nonzero word, so empty and live never coincide.
- **References.** A reference to a class value names the 16 bytes before its
  record. Readers use `address + 16` without loading the word there; writers
  copy the record bytes, and writing an empty value zeroes them. Top-level
  values keep a real word at that address, flat fields do not, and no reader
  depends on which. Sort-key adapters, which receive runtime slots holding a
  row's record address, convert to that form before calling compiled code.
- **Destructors.** The runtime passes `record - 16` to the `__del__` hook,
  which must not read the word.
- **Newtype construction.** A one-field class wrapping a class with a record
  is filled through a reference to the record, so `New` allocates the record's
  storage as the wrapped class would.

### Compiler and runtime

- `Ty::flat_field` names the rule; `Ty::field_bytes` and
  `ClassType::field_offset` give a field's width and offset.
- `lower_class` projects flat fields by offset (`class_field`,
  `referenced_record`); `read_typed_reference` and `write_record` implement the
  reference contract.
- `fix_record`, `release_record` and `record_fixes` (`src/codegen/inline.rs`)
  walk flat fields in place and emit no fix for them.
- The runtime `Field { at, flat }` iterator (`Record::slots`) reads, writes and
  clears a field by kind. Copy, relocate, release, hashing, equality, ordering
  and formatting go through it. The resumable release walk clears a finished
  flat record, and `Field::is_clear` lets a resumed pass skip it.
  `abandon_copy` releases the copied prefix without allocating.

### FFI

No change. A handle is still a `Box` holding one top-level slot, whose word
addresses the record; borrowed and owned arguments use the same descriptor
paths. Generated headers and `.plentyi` interfaces are byte-identical for a
library exporting nested classes.

### Measurements

Retired instructions (`perf stat -e instructions:u`, `--release`, mean of 5,
pinned to 8 cores). *Base* is main at 5e6e98a4, before stage 2; *main* is
0ec41c06, which adds the empty-slot release removal (issue #549).

| Program | Base | Main | Stage 2 |
|---|---|---|---|
| Three-level `Point`/`Segment`/`Path` workload | 5,033,105,263 | 5,033,105,267 | 3,700,365,927 (−26.5%) |
| Its move loop, per iteration | 1,840 | 1,840 | 1,302 (−29.2%) |
| Its list rows alone | 1,325,304,563 | 1,325,304,542 | 1,096,565,424 (−17.3%) |
| Nested two-field records copied through a loop | 282,667 | 281,066 | 280,637 |
| Constructor fixture (wrapped file) | 331,192 | 262,145 | 262,143 |
| Copies fixture, 5M wrapper cycles and 300k shelves | 5,612,022,549 | 3,868,322,585 | 3,570,485,254 (−7.7% vs main) |
| Constructor fixture, scaled | 2,225,242,405 | 505,241,987 | 505,241,982 |
| 300k shelves of nested records appended and destroyed | 3,387,022,898 | 3,363,322,395 | 3,065,484,601 (−8.9% vs main) |
| 200 loopback `connect`/`accept`/`close` pairs through `std.net` | 1,864,648 | 1,833,413 | 1,832,631 |

Code size for the three-level workload fell from 130,321 to 121,521 bytes of
`.text`. `compile_bench` (`aot`, 1000 × 10, pinned to 8 cores) retires 0.4%
more instructions (12.28 G to 12.33 G for 3 repetitions); its median times
(244–254 ms on main, 243–260 ms with stage 2) are within noise.

The first version of stage 2 made the shelves workload 7% slower than base.
The runtime release walk restarts at an owner's first field after each heap
child, and with flat fields each restart re-entered nested records and scanned
finished ones. Two runtime changes removed that cost:

- The walk frees a string in place instead of queueing it and restarting;
  a string owns nothing and runs no user code, so the order is unobservable.
  A `Shelf` with three strings is now walked once instead of four times.
- Record traversal decides a field's kind once per field, and walks into a
  flat record build its `Record` directly instead of re-deriving it through
  `Type::record`.

Per shelf (callgrind, 20k shelves), the whole program fell from 9,716
instructions on base to 8,658; the release walk itself fell from 1,760 to
1,210, and relocation is level with base (685 and 663). Compiled moves no longer
fix flat fields but still copy their bytes, so the per-move saving is the fixes,
not the copy.

## Stage 3: fewer runtime walks (issue #550)

Stage 2 left flat records copied by `memmove` and released and relocated by
the runtime's generic field walk. Profiles of the stage 2 workloads
(callgrind, 20k rows of three-level `Path` records) put the release walk at
27% of the program, list appends' `copy_payload` relocation at 16%, and the
`memmove` calls for 176-byte record copies at 9%. Each change below targets one
of those shares.

- **Inline copies.** Copies and clears of 33 to 192 bytes are 16-byte vector
  loads and stores instead of `memmove`/`memset` calls. All loads precede the
  stores, so overlapping copies stay correct. Larger records, such as the
  272-byte `Shelf`, still call `memmove`.
- **In-place moves.** A local moved into a call argument, with only operand
  pushes between the move and the call, is passed at its own address: the
  callee copies arguments on entry, so the snapshot copy was redundant.
  Inlined constructors, generator producers and portable SIMD operations keep
  argument addresses and still receive a copy (`inline::Moves`).
- **Descriptor plans.** A class descriptor with a record grows from 72 to 80
  bytes; byte 72 links a `Plan` with the word offsets of the strings the record
  owns (through flat fields), whether they are all it owns, and whether its
  bytes contain addresses into themselves. The runtime frees such a record's
  strings in one pass instead of walking its fields, skips relocation for a
  record without internal addresses, and skips per-row release for a list of
  records that own nothing. Records with `__del__` at any depth, or any other
  managed field, keep the walk, so destructor order is unchanged.

The plan is data rather than a compiled hook. Descriptors are defined lazily
while relocations are resolved, after the parallel function build; a compiled
release function would need every eligible class declared before that build.
For records owning only strings the data plan reaches the same result: no
per-field dispatch, one load and release per string.

### Measurements

Retired instructions (`perf stat -e instructions:u`, `--release`, mean of 5,
pinned to 8 cores). *Main* is 240cdd71.

| Program | Main | Stage 3 |
|---|---|---|
| Three-level `Point`/`Segment`/`Path` workload | 2,508,593,168 | 1,312,945,806 (−47.7%) |
| 2M moves of a `Path` through `keep` | 466,240,999 | 376,240,717 (−19.3%) |
| 300k `Path` rows appended and destroyed | 2,035,990,651 | 932,943,557 (−54.2%) |
| 300k shelves of nested records appended and destroyed | 2,380,500,554 | 2,108,859,497 (−11.4%) |
| Copies fixture, scaled | 3,024,103,704 | 2,756,512,591 (−8.8%) |
| Constructor fixture, scaled | 1,205,241,633 | 1,205,241,629 |
| Nested two-field records copied through a loop | 280,638 | 277,878 |
| Constructor fixture (wrapped file) | 262,142 | 262,144 |
| 200 loopback `connect`/`accept`/`close` pairs through `std.net` | 2,045,036 | 2,009,922 (−1.7%) |

Wall-clock medians (15 runs): the three-level workload 254 to 140 ms, `Path`
rows 187 to 116 ms, moves 68.7 to 27.5 ms, shelves 246 to 222 ms, copies 313 to
291 ms. Per `Path` row (callgrind, 20k rows) the program fell from 5,819 to
3,016 instructions: destroying the list, string frees included, from 1,611 to
110, and the append's `copy_payload` from 920 to 204. Per shelf, destroying
the list fell from 1,252 to 700 instructions, because its `Label` fields own
only strings.

Inline copies cost code size: `.text` grew 0.2% to 1.7% across examples
(`classes` 81,121 to 81,313 bytes, `http_server_bench` 556,705 to 566,257,
`semantic_search` 1,768,878 to 1,773,166, `actor_echo` 252,641 to 256,193) and
1.3% to 3.2% for the record workloads. A 256-byte limit sped up none of the
workloads and grew `actor_echo` by a further 2.5%; a 128-byte limit cost the
moves workload 8%. `compile_bench` (`aot`, 1000 × 10) retires 0.18% more
instructions (80.53 G to 80.68 G); its median times (332–340 ms on main,
331–336 ms with stage 3) are within noise.

### Remaining

Shelves keep part of their walk cost: a `Shelf` holds ranges and sums, whose
payload addresses depend on the active variant, so it is neither string-only
nor free of internal addresses. Per shelf, relocation still costs about 930
instructions and the 272-byte record copy still calls `memcpy`. A plan entry
per internal address, tested against the sum's tag, would cover relocation.
