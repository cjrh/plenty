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

Retired instructions (`perf stat -e instructions:u`, `--release`, mean of 5)
against main at 5e6e98a4:

| Program | Before | After |
|---|---|---|
| Three-level `Point`/`Segment`/`Path` workload (moves, list rows, borrows) | 5,033,105,519 | 3,886,508,416 (−22.8%) |
| Its move loop alone, per iteration | 1,840 | 1,302 (−29%) |
| Its list rows alone | 1,325,304,782 | 1,282,707,905 (−3.2%) |
| Nested copies, 200× scaled fixture | 5,612,022,727 | 5,848,707,786 (+4.2%) |
| Shelves of nested records appended and destroyed | 3,387,022,996 | 3,623,707,735 (+7.0%) |
| Nested two-field records copied through a loop | 282,905 | 283,067 |
| Constructor fixture (wrapped file) | 331,431 | 331,429 |
| 200 loopback `connect`/`accept`/`close` pairs through `std.net` | 1,864,882 | 1,864,158 |

Code size for the three-level workload fell from 130,321 to 121,521 bytes of
`.text`. `compile_bench` (`aot`, 1000 × 10, pinned to 8 cores) showed no
difference beyond noise.

The two regressions are runtime walks. A resumable release restarts at the
first field after each heap child; it now scans each finished flat record's
words to skip it and re-enters an unfinished one, where the address layout
tested one zero word. Relocation and copying also test each field's kind.
Together these cost about 590 instructions per released `Shelf` (callgrind).
Moving those walks into compiled code for known record types would remove
most of it. Compiled moves no longer fix flat fields, but still copy
their bytes, so the per-move saving is the fixes, not the copy.
