# Classes: fixed-layout records

`Class(arguments)` and its `Class.new(arguments)` alias return the instance
directly, with identical visibility and argument types. Instance storage is
inline, so construction never allocates. Allocations inside argument expressions
or an initializer keep their own `Result`. `new` is a reserved class member.

An explicit `__init__` may return `Result[(), AllocError]`. Then both forms return
`Result[Class, AllocError]`; `?` in initialization propagates through the constructor.
Failure drops initialized fields and remaining arguments, but skips the class's
`__del__`. The custom destructor becomes active only after successful initialization.
Every successful return must initialize every field. An explicit `return Err(...)`
may exit earlier. Other user-defined initializer error types are not yet supported.

`class` declares a concrete record with typed fields and associated methods.
It provides familiar Python-shaped organization without inheritance, dynamic
attributes, class variables, properties, or runtime method lookup.

```python
class Point:
    x: i64
    y: i64

    def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y

    def shift(self: &mut Point, amount: i64) -> ():
        self.x = self.x + amount
        self.y = self.y + amount

def main() -> ():
    mut point = Point(3, 4).unwrap()
    point.shift(2)
```

Without `__init__`, the compiler generates a positional constructor taking all
fields in declaration order. An explicit `__init__` returning `()` or
`Result[(), AllocError]` overrides it. Every field must be definitely initialized
on every successful exit;
branches merge their initialization sets, and a loop alone cannot establish
initialization because it may run zero times. Already initialized fields may be
read. Passing or borrowing the whole partially initialized instance is rejected.
Initialization cannot be delegated to another method. Field defaults, keyword
arguments, static methods, and constructor overloading are deferred.

A method's first parameter is `self`. Bare `self` means `self: &Class` in
ordinary methods and `self: &mut Class` in `__init__` and `__del__`. Other
parameters and every return require explicit types. Mutating ordinary methods
declare `self: &mut Class`; calling them requires a mutable owner or exclusive
reference. Calls borrow the receiver and reborrow reference arguments for the
duration of the call. Temporary owned receivers are supported without permitting
escaping references.

Every class instance moves on assignment and owned argument passing, including
records containing only integers. `copy(instance)` recursively duplicates owned
fields, but is rejected for recursive data or if any nested value has custom destruction.
Field reads copy immutable values; owned fields must be observed, explicitly
copied, or borrowed. Partial moves out of classes are deferred. Structural
equality compares the nominal type and field values; printing produces
`Point(x=3, y=4)`. Automatic equality and formatting reject recursive data.
These operations do not invoke user-defined magic methods.

`&point.x` and `&mut point.x` borrow stable field slots, including nested
class fields. Loans record field-index paths from the root: different sibling
fields can be borrowed or mutated independently. Whole-root and ancestor-path
access still overlaps every descendant. Collection-valued
fields support in-place updates such as `record.items.append(value)`.
Whole-instance replacement through `*reference = instance` is rejected for
classes, including class-valued field references; assign an owning binding or
a named field instead. This also prevents a destructor from replacing its dying
receiver through an alias.

Fields may contain classes, enums, and collections, but not references,
generators, or unit. Forward declarations, aliases, and
[recursive owners](30-recursive-data.md) through `Box` or a collection are
supported. Methods become statically resolved native functions.

An instance keeps its fields inline, in the storage of whatever owns it: a
local's frame slot, a containing record, or a collection's buffer. Its 128-bit
value word addresses that storage. Each field uses a typed 16-byte slot plus the
field's own inline bytes. A class with `__del__` keeps one more word before its
fields: whether initialization completed, and how many observers currently
borrow it through retained copies of the value word. Releasing an observer only
decrements that count; releasing the owner runs the destructor, then drops the
fields in declaration order. The destructor adapter is linked into the class's
type metadata. Moving an instance copies its storage. Inline storage is limited
to 64 KiB per type. This representation is not a public FFI layout guarantee.
The compiler caches complete graph properties and layouts.
