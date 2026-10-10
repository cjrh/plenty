# Group fields and methods in a class

A class is a record with typed fields. Without `__init__`, its constructor takes
the fields in declaration order:

```plenty
class Point:
    x: i64
    y: i64

    def squared_length(self) -> i64:
        self.x * self.x + self.y * self.y

    def shift(self: &mut Point, amount: i64) -> ():
        self.x = self.x + amount
        self.y = self.y + amount

def main() -> Result[(), Failure]:
    mut point = Point(3, 4)
    print(point.squared_length())?
    point.shift(1)
    mut changed = copy(point)?
    changed.x = 20
    print(point)?
    print(changed)?
    Ok(())
```

```output
25
Point(x=4, y=5)
Point(x=20, y=5)
```

Ordinary methods borrow `self` read-only by default: `self` is shorthand for
`self: &Point` here. A method that changes fields declares `self: &mut Point`.
Calling it requires a `mut` binding or an exclusive reference.

Instances store their fields inline: `Point(3, 4)` needs no allocation or `?`.
Copying list fields can still fail to allocate.

Class instances move even when their fields are all integers. `other = point`
transfers ownership; `copy(point)` requests an independent instance. Integer
and string fields can be read directly. Owned fields such as lists or other
classes must be borrowed, observed, or explicitly copied. Equality compares
fields; printing shows the class name and fields.

There is no inheritance, dynamic attribute creation, or class-variable syntax.
Fields currently have no default values, and calls use positional arguments.
