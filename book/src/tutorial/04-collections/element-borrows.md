# Borrowing collection elements

Borrow an element when you want to inspect or change it without moving its owner:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut counts = [1, 5]?
    value = &mut counts[0]
    *value = 2
    print(counts)?
    Ok(())
```
```output
[2, 5]
```

The loan ends after its last use. While an element reference is live, the
collection cannot grow, remove entries, or move. An out-of-range index traps.
Dictionaries lend a value through
[`get_mut`](dictionaries.md), which returns an `Option` instead of trapping on
a missing key.

Two list elements written as different number literals are different places, so
both can be borrowed at once:

```plenty
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut scores = [10, 20, 30]?
    first = &mut scores[0]
    last = &mut scores[2]
    *first = *first + *last
    *last = 0
    print(scores)?
    Ok(())
```
```output
[40, 20, 0]
```

The compiler proves only this literal case. Any other index, such as `i` or
`-1`, may name the same element, so its loan covers the whole list:

```plenty-error
def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut scores = [10, 20, 30]?
    i = 2
    first = &mut scores[0]
    last = &mut scores[i]
    *first = *first + *last
    Ok(())
```
```error
while `scores[0]` is exclusively borrowed
```

Dictionary entries are never separated this way. To exchange two list elements
at computed indices, [`swap`](reordering.md) them instead.

Borrowed loops always give references, including when the elements are numbers.
Printing and comparison inspect those references directly. Use `*` when a
function takes a value or a new collection needs the element itself:

```plenty
def count_eq[T](values: &list[T], target: &T) -> Result[i64, OverflowError[i64]]:
    mut count = 0
    for value in values:
        if value == target:
            count = count + 1
    Ok(count)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    values = [1, 2, 1]?
    target = 1
    print(count_eq(&values, &target)?)?
    for value in &values:
        print(*value)?
    Ok(())
```

```output
2
1
2
1
```

The same body works for a `list[str]`. Shared dictionary `items()` loops likewise
give `&K` keys and `&V` values; mutable views give `&mut V` values. A borrowed
collection stays protected against changes throughout its loop.

## Borrow part of a list

`&values[a:b]` borrows a run of elements in place. This *slice view* copies
nothing, so unlike a [copying slice](slices.md) it needs no `?`. A function
takes a view as `&[T]`, or as `&mut [T]` to change the elements:

```plenty
def total(values: &[i64]) -> Result[i64, OverflowError[i64]]:
    mut sum = 0
    for value in values:
        sum += *value
    Ok(sum)

def reset(values: &mut [i64]) -> ():
    for value in values:
        *value = 0

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut scores = [70, 85, 90, 65, 88]?
    print(total(&scores)?, total(&scores[1:])?)?
    reset(&mut scores[:2])
    print(scores)?
    Ok(())
```

```output
398 328
[0, 0, 90, 65, 88]
```

The positions clamp as a copying slice's do. A view supports `len`, indexing,
narrower views, loops, and `==` with another view, and prints as a list of its
elements. An exclusive view also takes indexed assignment and
[`swap`](reordering.md#exchange-two-elements). A `&[T]` parameter also takes a whole list as `&scores`, which is
a view that copies nothing. A view borrows its
run of the list, and the list cannot grow while the view is live. Two exclusive
views of one list can be live together when one stops where the other starts:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut scores = [70, 85, 90, 65, 88]?
    half = len(scores) // 2
    low = &mut scores[:half]
    high = &mut scores[half:]
    low[0] = high[0]
    print(scores)?
    Ok(())
```

```output
[90, 85, 90, 65, 88]
```

Views whose ranges may overlap, such as `&mut scores[:3]` and `&scores[2:]`,
conflict. Byte buffers have views too. A view cannot
be returned or stored yet; the
[slice reference](../../design/language/04-expressions/slices.md#slice-views)
lists its operations and limits.

## Borrow part of a string

`&text[a:b]` borrows part of a string the same way. Its type is `&substr`, and
its positions count characters, as a [copying string slice](slices.md#text)'s
do. Loop over it, compare it with a string, or search it without copying:

```plenty
def count(text: &substr, letter: &str) -> Result[i64, OverflowError[i64]]:
    mut n = 0
    for c in text:
        if c == letter:
            n += 1
    Ok(n)

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    line = "name: Ada Lovelace"
    value = &line[6:]
    print(value, len(value))?
    print(&line[:4] == "name", "Love" in value, count(line, "a")?)?
    first = value[:3]?
    print(first)?
    Ok(())
```

```output
Ada Lovelace 12
True True 3
Ada
```

A function takes a substring as `&substr`. Passing a whole string as `line`
implicitly borrows it for the call; `&line` or a literal also works. Copy the
text with `value[a:b]?` when you need an owned `str`,
for example to call a `str` method or keep it after the string changes. The
[substring view reference](../../design/language/04-expressions/slices.md#substring-views)
lists every operation.

To inspect the UTF-8 encoding, borrow its bytes directly. The byte view uses byte
positions and loops over `&u8`; the text view still loops over scalars:

```plenty
import std.bytes

def main() -> Result[(), AllocError | IoError]:
    text = "café"
    ending = &text[3:]
    bytes = ending.as_bytes()
    print(len(ending), len(bytes))?
    for byte in bytes:
        print(*byte)?
    owned = std.bytes.from_text(ending)?
    print(owned)?
    Ok(())
```

```output
1 2
195
169
b"\xc3\xa9"
```

`as_bytes()` allocates nothing and keeps the string borrowed until the byte
view's last use. `from_text` makes an independent fallible byte-buffer copy.
