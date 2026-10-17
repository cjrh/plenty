# References, ownership, and explicit copying

> **Status: Implemented.** Model A was accepted and shipped: mutable values
> move, borrows are checked, and copying is explicit. The
> [ownership reference](../../design/language/05-ownership/index.md) owns the
> contract. This superseded the original ownership proposal, which copied
> collections on update; that page was removed. Model B below is the rejected
> alternative.
>
> **Shipped beyond this text:** [returned references](../../design/language/05-ownership/04-returned-references.md),
> field and element projections, [reference rebinding](../../design/language/05-ownership/03-reference-rebinding.md),
> [borrowed matching](../../design/language/05-ownership/05-borrowed-matching.md),
> user destructors (`__del__`), and `Box[T]` as a public owner type. An `&T`
> parameter borrows an owned argument implicitly. Allocating operations,
> including `copy`, return `Result`.
>
> **Remaining:** stored references, relationships among several reference
> parameters, and disjoint-index loans (B11).

## Motivation and current costs

Before this migration, assignment retained a collection handle. Mutation then copied
the collection's entries and retains nested values before changing the copy.
The old append and indexed-update paths did this even with only one
language-visible owner, making repeated appends quadratic. Updates now happen in
place; explicit copy recursively duplicates mutable contents. Assignment, arguments, indexing, and enum payload
projection all need review together; changing assignment alone is insufficient.

We want collection duplication to be explicit, while retaining fast compilation,
typed function contracts, automatic reclamation, and one public `str` type.

Python assignment binds a name to an object; it does not copy integers or strings
either. Immutability makes sharing those objects safe. Plenty can preserve that
observable behavior while storing fixed-width scalars directly and sharing
immutable string storage. String rebinding must remain distinct from mutation of
the string's bytes. An exclusive reference to a string binding could replace
that binding without making the string object mutable.

## Two coherent models

### A. Transfer mutable values; borrow explicitly

- Fixed-width scalars copy cheaply. Immutable `str` storage can be shared without
  copying its bytes or introducing another public string type.
- Assignment and owned arguments transfer collections, generators, and aggregates
  that contain owned mutable values. Reading a transferred binding is an error.
- `&T` lends read access, and `&mut T` lends exclusive write access. The owner
  survives a borrow but conflicting access is prohibited until its last use.
- `copy(value)` explicitly produces an independent value. For nested mutable
  collections this recursively duplicates owned mutable contents; immutable
  strings may share storage. This differs from Python's shallow `copy`.
- Resource types such as generators have no general copy operation.

For example:

```plenty
def append_three(values: &mut list[i64]) -> Result[(), AllocError]:
    values.append(3)?
    return Ok(())

def main() -> Result[(), AllocError]:
    mut original = [1, 2]?
    mut changed = copy(original)?
    changed.append(3)?                # original remains [1, 2]
    append_three(&mut original)?      # changes original without copying it
    mut destination = original        # transfers ownership
    # print(original)                 # error: moved binding
    return Ok(())
```

The copy builtin above takes a temporary shared borrow. Ordinary functions expose
their borrowing or ownership contracts in signatures; the compiler does not infer
cross-function contracts by examining implementations. `mut` permits writing
through an owner or exclusive access path, not creating another unrestricted
writer. References to whole bindings and references to their collection elements
are different places and must be checked accordingly.

This is the accepted direction for static checking and explicit costs. Its
main usability cost is that collection assignment does not preserve both names.
It needs no lifetime syntax. `Box[T]` later became the one public owning pointer,
for recursive data.

### B. Shared mutable objects, like Python

- Collection assignment and arguments retain handles to the same mutable object.
  Both names remain usable; mutations are visible through every alias.
- `copy(value)` can naturally be shallow, like Python; nested mutable objects
  remain shared. Full independence then needs a separate deep-copy operation.
- Binding rebinding permissions, permissions to mutate an object through a handle,
  and guarantees that an object cannot change must be distinguished. An immutable
  binding does not by itself make a shared object immutable.
- Reference counting handles object lifetime, but does not establish exclusive
  access or prevent references into relocated buffers from dangling.

For public references into shared mutable collections, choose an explicit policy:
conservatively reject conflicting accesses through all possible aliases, check
borrows at runtime, or use stable handles that resolve accesses without exposing
raw element addresses. Stable handles still require rules for removed/replaced
elements and do not themselves provide exclusive access. A handle must not lower
to a native exclusive reference without establishing exclusivity.

Runtime borrow guards are a practical implementation for unrestricted shared
owners plus temporary direct references. Aliases may coexist; access that conflicts
with a live guard fails at runtime. This adds checks and potential runtime errors,
including in single-threaded code with nested function calls. It is not a general
compile-time borrow-checking guarantee. A static alias analysis can remove some
checks or reject more programs, but should not require whole-program analysis to
understand a function's contract.

This model is viable if preserving Python's mutable aliasing is the priority.
Future recursive structs would also require a policy for ownership cycles;
reference counting alone would not reclaim them.

## Migration requirements

1. Define operations on source places in a typed control-flow graph before native
   lowering. Record reads, writes, moves, borrows, reborrows, and destruction.
2. Separate lifetime ownership from permission to access. Internal retained
   temporaries and reference counts are not proof that a borrow is exclusive.
3. Specify argument evaluation and temporary loans, branch joins, loops, and
   early exits. Use last-use liveness rather than ending every loan at block end.
4. Define indexing and match projection without silently copying mutable payloads.
   Initially reject unsupported moves out of indexed storage; add explicit removal
   operations separately. Match may borrow a scrutinee or consume it, with field
   cleanup determined by that choice.
5. Define iteration under mutation. Model A can borrow the collection during a
   loop; model B needs guarded iteration, invalidation detection, or documented
   mutation behavior. Do not preserve hidden snapshots by copying implicitly.
6. Keep public references out of stored fields, returned values, and suspended
   generators in the first implementation. Add each only with an explicit lifetime
   contract; reject local references that escape or cross an unsupported yield.
   (Returned references have since shipped under a single-reference-parameter
   rule.)
7. Test conflicting alias access, collection resizing with live element borrows,
   reference reassignment, reborrows, branch-dependent moves, early exits, and
   cleanup. A source check must run independently of Cranelift verification.

These requirements are met: the borrow checker runs on a source control-flow
graph with last-use loans, before native lowering.

## Sources

- [Python assignment and copying](https://docs.python.org/3/library/copy.html)
- [Rust compile-time and runtime borrowing](https://doc.rust-lang.org/book/ch15-05-interior-mutability.html)

The selected model borrows specific ideas rather than adopting either language's
complete ownership model.
