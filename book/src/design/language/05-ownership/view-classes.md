# Classes that borrow one source

A class with a direct `&T`, `&mut T`, `&[T]`, `&mut [T]` or `&strview`
field is a **view class**. It keeps one checked origin borrowed until the
instance and its derived results are last used. Its ordinary fields retain
normal ownership and cleanup. Construction adds no heap allocation.

```plenty
class Cursor:
    data: &[i64]
    position: i64
    def remaining(self: &Cursor) -> &[i64]:
        &self.data[self.position:]

def suffix(data: &[i64]) -> Cursor:
    Cursor(data, 1)

def main() -> Result[(), AllocError | IoError]:
    values = [3, 5, 8]?
    cursor = suffix(&values)
    remaining = cursor.remaining()
    print(remaining[0])?
    Ok(())
```
```output
5
```

Use a free function over a builtin view when there is no state to name. A
view class is useful when borrowed data belongs with state or several methods.
Its methods retain the usual bounds and arithmetic checks.

## One origin and fixed fields

Every borrowed field must derive from the first loan-carrying constructor
parameter after `self`. A generated constructor uses the first borrowed field
argument. Multiple shared fields require an explicit initializer taking one
borrowed parent and deriving the fields from it. Separate fieldwise constructor
inputs have independent formal origins; this version does not infer their
relationship from a caller's aliases. Argument names and written evaluation
order do not change the origin parameter. Independent borrowed sources cannot
share one view class.

Each successful initializer path must initialize every field. Borrowed fields
are initialized exactly once, outside loops, through the initializer's own
`self`. They cannot subsequently be rebound, replaced, swapped or moved out.
Mutation through an exclusive borrowed field changes the referent, not the
stored descriptor. Whole-instance replacement through a reference is rejected;
a named `mut` holder can be reassigned within its original source.

Several shared fields may borrow one origin. An exclusive view class has
exactly one exclusive borrowed field and no other borrowed field. It always
moves. A shared view class copies on ordinary use only when explicitly
`@copy` and its ordinary fields also copy. Every copy keeps the source borrowed.

Borrowed fields refer to ordinary non-borrowing types. Other fields use the
usual storage rules; they cannot contain another view class or borrowing
frame. A view class cannot declare `__del__`, `__enter__` or `__exit__`.
Ordinary owned fields still drop normally, including partial initialization
on an error path. Borrowed fields never retain or destroy their referents.

## Calls and receiver stability

Functions may accept a view class by value, `&View` or `&mut View`, including
concrete generic specializations. A returned view class follows the
[first-origin return rule](04-returned-references.md). A consuming parameter
may forward borrowed data but cannot return an address into its own record.

A borrowed method result keeps both the named receiver and its source alive.
This applies to shared methods too: the receiver cannot move, drop, be
reassigned or be mutably borrowed until the result's last use. A method may
return a builtin reference/view, another view class, or an allowed success
wrapper. A result borrowing a temporary cannot be stored or returned beyond
its full expression; bind the receiver or source first.

## Wrappers and storage boundaries

One `Option[View]` or `Result[View, E]` success layer is supported, including
matching, propagation and extraction. The error cannot borrow. Nested success
wrappers and borrowing a sum holder, such as `&Option[View]`, are rejected.
Records keep complete borrowed descriptors; sum tags do not consume view
length bits. Layout remains the private inline-class ABI.

View classes belong in local bindings, ordinary calls and scoped worker
inputs. A scoped worker keeps the source and any receiver dependencies
borrowed through its lexical join boundary, including after explicit `join()`.
Worker results must be owned. View classes cannot enter heap collections,
tuples, ordinary class fields, user enums, boxes, shared handles, channels,
actors, erased environments or detached jobs. They cannot cross C exports or
derive serialization. A reusable local closure may borrow a named view class;
it cannot capture the instance by value.

View-class parameters and live view-class locals cannot enter suspended
generator frames. Yielded view classes and pipeline adapter results are also
unsupported. View classes can supply the ordinary exclusive
[`next` iterator protocol](../09-iteration/08-user-iterators.md), whose borrowed
items keep the strict receiver lifetime. See the
[approved design](../../../proposals/completed/view-classes.md) for rationale and
alternatives.
