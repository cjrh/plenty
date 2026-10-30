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

def main() -> Result[(), AllocError | IoError]:
    mut point = Point(y=4, x=3)
    print(point.squared_length())?
    point.shift(amount=1)
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
An explicit `self: Point` instead takes ownership; see
[consuming methods](consuming-methods.md) for state transitions and finalization.

Methods can share names with integer built-ins. A class receiver selects its
own method, including its parameter and return types:

```plenty
class Counter:
    total: i64

    def wrapping_add(self: &Counter, other: i64) -> i64:
        self.total + other

def main() -> Result[(), IoError]:
    counter = Counter(1)
    print(counter.wrapping_add(2))?
    Ok(())
```

```output
3
```

This method uses ordinary checked addition; its name does not change that.

If you forget the receiver annotation, the error names the required signature:

```plenty-error
class Counter:
    value: i64
    def increment(self) -> ():
        self.value = self.value + 1

def main() -> ():
    pass
```

```error
declare `self: &mut Counter`
```

Change the parameter to `self: &mut Counter`, as in `shift` above.
The lifecycle methods `__init__` and `__del__` give bare `self` exclusive
access automatically.

Instances store their fields inline: `Point(3, 4)` needs no allocation or `?`.
Copying list fields can still fail to allocate.

By default, class instances move even when their fields are all integers.
`other = point` transfers ownership; `copy(point)` requests an independent
instance. Integer and string fields can be read directly. Owned fields such as
lists or other classes must be borrowed, observed, or explicitly copied.
Equality compares fields; printing shows the class name and fields.

## Copy small value types

Write `@copy` on the line before `class` when instances are plain values, like
integers. Assignment, argument passing, returning, and storing in a collection
then copy the instance and leave the original usable:

```plenty
@copy
class Money:
    cents: i64
    currency: str

def add(a: Money, b: Money) -> Money:
    Money(a.cents + b.cents, a.currency)

def main() -> Result[(), IoError]:
    price = Money(250, "EUR")
    tax = Money(50, "EUR")
    paid = add(price, tax)
    mut discounted = price
    discounted.cents = 200
    print(price, paid, discounted)?
    Ok(())
```

```output
Money(cents=250, currency="EUR") Money(cents=300, currency="EUR") Money(cents=200, currency="EUR")
```

Every field must itself copy: a scalar, `str`, another `@copy` class, or a tuple
or enum of those. Copying duplicates the inline fields and never allocates, so
it needs no `?`. A field that owns storage is rejected:

```plenty-error
@copy
class Basket:
    items: list[str]

def main() -> ():
    pass
```

```error
every field of a `@copy` class must copy
```

A `@copy` class also cannot define `__del__` or take type parameters. Keep the
default for instances that own resources or should be used only once; see
[copy classes](../../design/language/03-types/classes.md#copy-classes).

## Hide secrets when printing

Printing a class shows every field, so a password or key reaches any log that
prints its holder. Write `@redacted` before `class` or `enum` to replace the
contents with `(@redacted)` wherever the value is printed or formatted, however
deeply it is nested:

```plenty
@redacted
class Password:
    text: str

class Login:
    user: str
    password: Password

def main() -> Result[(), AllocError | IoError]:
    login = Login("ada", Password("hunter2"))
    print(login)?
    print(str.repr(login.password)?)?
    print(login.password.text == "hunter2")?
    Ok(())
```

```output
Login(user="ada", password=Password(@redacted))
Password(@redacted)
True
```

The fields stay readable in code, so redaction prevents accidental printing
rather than enforcing secrecy; see
[redacted types](../../design/language/03-types/classes.md#redacted-types).

## Limits

There is no inheritance, dynamic attribute creation, or class-variable syntax.
Fields currently have no default values. Constructors and methods accept named
arguments; an explicit `__init__` can give its parameters defaults.
