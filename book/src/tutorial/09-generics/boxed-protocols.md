# Store different implementations of a protocol

An [enum wrapper](../heterogeneous-collections/lists.md) works well when your
program knows all the alternatives. A protocol box lets implementations be
added independently: the collection only names the operations it needs.

```plenty
protocol Counter:
    def current(self) -> i64:
        pass

    def increment(self: &mut Counter) -> Result[(), OverflowError[i64]]:
        pass

class Count:
    value: i64

    def current(self) -> i64:
        self.value

    def increment(self: &mut Count) -> Result[(), OverflowError[i64]]:
        self.value += 1
        Ok(())

enum Flag:
    Off
    On

    def current(self) -> i64:
        match self:
            case Flag.Off:
                0
            case Flag.On:
                1

    def increment(self: &mut Flag) -> Result[(), OverflowError[i64]]:
        *self = Flag.On
        Ok(())

def main() -> Result[(), AllocError | IoError | OverflowError[i64]]:
    mut items: list[Box[Counter]] = []?
    items.append(Box[Counter](Count(10))?)?
    items.append(Box[Counter](Flag.Off)?)?
    for item in &mut items:
        item.increment()?
        print(item.current())?
    Ok(())
```
```output
11
1
```

Each `Box[Counter](value)?` moves its input into one heap allocation. The list
stores owners with a common interface. Its mutable loop borrows each owner, so
`increment` and `current` operate on the same instance. No match or cast is
needed. Use `&items` for a loop that only calls shared methods.

The protocol fixes each method's contract. `Count.increment` can overflow, so
`increment` returns `Result[(), OverflowError[i64]]`, and `Flag` declares the
same result even though it never fails.

The `Box` boundary is explicit: `list[Counter]` is not a stored value type, and
`Box(Count(10))?` preserves the concrete `Count` type. Allocation failure drops
the consumed input. Appending can also fail and consumes its new box under the
ordinary list contract.

## Dictionary values and generic readers

The same owner type works as a dictionary value and through a protocol bound:

```plenty
protocol Label:
    def label(self) -> str:
        pass

class Message:
    text: str

    def label(self) -> str:
        self.text

enum Status:
    Ready

    def label(self) -> str:
        return "ready"

def read[T: Label](value: &T) -> str:
    value.label()

def main() -> Result[(), AllocError | IoError]:
    items: dict[str, Box[Label]] = {
        "message": Box[Label](Message("hello"))?,
        "status": Box[Label](Status.Ready)?,
    }?
    for name, item in items.items():
        print(name, read(item))?
    Ok(())
```
```output
message hello
status ready
```

Borrowing avoids copying the boxes. Their concrete payloads are destroyed when
their owners leave scope, including any custom cleanup.

A box exposes only its protocol's methods. This initial interface supports
shared and mutable receivers, fixed protocol arguments, ordinary parameters,
and owned results. Methods with their own type parameters, consuming receivers,
or returned borrows cannot be boxed. Protocol boxes cannot be copied, used as
set elements or dictionary keys, or transferred to worker threads. Payloads
containing channel senders or receivers also need concrete owners: erasing them
would hide endpoints from the compiler's checks for waits that could deadlock. See the
[reference](../../design/language/07-generics/boxed-protocols.md) for the complete
limits.
