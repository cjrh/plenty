# User iterators

A class is an iterator when it has an accessible ordinary method with the
shape `next(self: &mut ClassName) -> Option[T]` and no other parameters.
Generic classes use their concrete specialization. No base class, decorator,
associated type or `__iter__` method is required. The method's ordinary
visibility, type checking, bounds checks and return-origin rules apply.

Each loop condition calls `next` once. `Some(item)` enters the body;
`Nothing` ends the loop. The protocol adds no allocation or runtime dispatch.
The method controls its own exhaustion behavior: unlike builtin generators,
the compiler does not promise that a later explicit call after `Nothing`
also returns `Nothing`.

## Owning and lending

`for item in source` evaluates the source once and copies or moves it into
the loop's hidden mutable owner. That owner is cleaned up on exhaustion,
`break`, return or propagated failure. Ordinary copy semantics still apply
to `@copy` classes. The original binding need not be `mut` when it transfers
into an owning loop.

`for item in &mut source` advances a named mutable iterator in place. An
exclusive reference parameter can be used directly as the source. A lent
loop preserves progress after `break` and never destroys the caller's
iterator. A shared source cannot advance. The source's exclusive loan lasts
through the loop, so the body cannot independently advance, move, reassign
or borrow the iterator.

`next(source)` and `next(&mut source)` call the same ordinary method on a
mutable named place; `source.next()` remains ordinary method syntax. The
builtin form requires one argument. It does not consume an iterator or
silently make an immutable owner mutable.

## Borrowed items are streaming items

An item may be owned, an ordinary reference, a builtin view or a
[single-origin view class](../05-ownership/view-classes.md), subject to the
ordinary restrictions on success wrappers. A borrowed result follows the
method's strict receiver lifetime: both the iterator and its source remain
borrowed until the item's last use. This includes shared items. Advancing the
iterator while an earlier item is still live is rejected, even if both items
would refer to unchanged external data. There are no lifetime annotations
and no relaxed shared-item rule.

```plenty
class Cursor[T]:
    data: &[T]
    position: i64
    def next(self: &mut Cursor[T]) -> Option[&T]:
        if self.position >= len(self.data):
            return Nothing
        index = self.position
        # position < len(data) <= i64.MAX, so increment cannot overflow.
        unchecked:
            self.position += 1
        Some(&self.data[index])

def main() -> Result[(), AllocError | IoError]:
    values = [3, 5, 8]?
    mut cursor = Cursor(&values, 0)
    first = next(&mut cursor).unwrap()
    print(*first)?
    # first is no longer used, so advancing is allowed.
    for value in &mut cursor:
        print(*value)?
    Ok(())
```
```output
3
5
8
```

Keeping a previous item in an outer `Option` does not permit carrying it
through another iteration. The same conflict check applies across `continue`
and backedges. After `break`, an item from a lent iterator may remain live
while the original iterator remains stable. An item from an owning loop
cannot outlive its hidden iterator, even when that iterator has no managed
fields. A borrow of an iterator's owned fields is never promoted to the
lifetime of unrelated external input.

## Consumers and limits

User iterators work in loops, eager comprehensions, `enumerate`, `zip` and
local `iter(source)` pipelines. Owned tuples support ordinary loop unpacking.
Borrowed items retain their loans through observation and adapter callbacks;
copy their contents explicitly when building an owning collection, for example
`[*item for item in &mut cursor]?`. Collection storage of references or view
classes remains rejected. The direct collection constructors retain their
builtin input contracts; use a comprehension or a typed pipeline `collect`
to collect user sources. `chain` accepts matching borrowed items from user
iterators. Each item retains the selected iterator's receiver and source loans;
chaining does not allow retaining it across another advance. The second source
is constructed immediately and starts advancing after the first ends.

Owned iterator state may enter an owned pipeline's `.generator()` conversion.
The existing [generator suspension rules](03-generators.md) still apply:
view classes and live receiver borrows cannot be suspended, and borrowed-item
generator conversion remains restricted. Having a `next` method grants no
heap storage, worker transfer, erasure or suspension permission. Unsupported
sources for `reversed`, parallel pipelines, `flat_map` and builtin collection
operations retain their existing checks; there is no implicit container-to-
iterator factory protocol or async iteration.
