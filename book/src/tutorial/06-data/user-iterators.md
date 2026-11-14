# Advance a borrowed cursor

Use a generator when each step produces an owned value. When a cursor lends
an item from borrowed input and also keeps progress, give its
[view class](../../design/language/05-ownership/view-classes.md) an exclusive
`next` method returning `Option`.

```plenty
class Cursor:
    data: &[i64]
    position: i64
    def next(self: &mut Cursor) -> Option[&i64]:
        if self.position >= len(self.data):
            return Nothing
        index = self.position
        # position < len(data) <= i64.MAX, so increment cannot overflow.
        unchecked:
            self.position += 1
        Some(&self.data[index])

def main() -> Result[(), AllocError | IoError]:
    values = [2, 4, 6]?
    mut cursor = Cursor(&values, 0)
    for value in &mut cursor:
        print(*value)?
        break
    # The lent loop kept the cursor's progress.
    for value in &mut cursor:
        print(*value)?
    Ok(())
```
```output
2
4
6
```

`Some` supplies the item and `Nothing` ends the loop. A loop over
`Cursor(&values, 0)` owns its cursor; `&mut cursor` lends the named cursor
and leaves it available afterward. `next(&mut cursor)` advances it once.

An item borrows the cursor as well as the input. Use the item before asking
for another; keeping the first item for later and advancing again is a
conflicting borrow. This conservative rule also applies to shared items.
The source list cannot resize or move while the cursor or its results still
borrow it. Bounds checks remain in place on `self.data[index]`.

Use `[*value for value in &mut cursor]?` when you need an owned list of the
remaining integers. See [user iterators](../../design/language/09-iteration/08-user-iterators.md)
for generic cursors, local pipelines and storage limits.
