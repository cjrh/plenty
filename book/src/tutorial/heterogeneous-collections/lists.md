# Lists of different kinds of values

A list has one element type, but that type can describe alternatives. Use a
wrapper enum when the alternatives form a concept in your program and share
operations. Use an anonymous sum when a local choice of whole types is enough.

## Give the alternatives a shared interface

A message and a status can both appear in a feed. `FeedItem` names that concept
and keeps the dispatch in its `label` method:

```plenty
class Message:
    text: str

    def label(self) -> str:
        self.text

enum Status:
    Ready
    Busy

    def label(self) -> str:
        match self:
            case Status.Ready:
                "ready"
            case Status.Busy:
                "busy"

enum FeedItem:
    Message(Message)
    Status(Status)

    def label(self) -> str:
        match self:
            case FeedItem.Message(message):
                message.label()
            case FeedItem.Status(status):
                status.label()

def main() -> Result[(), AllocError | IoError]:
    mut items: list[FeedItem] = []?
    items.append(FeedItem.Message(Message("hello")))?
    items.append(FeedItem.Status(Status.Ready))?
    for item in &items:
        print(item.label())?
    Ok(())
```
```output
hello
ready
```

The wrapper can carry class instances, enum values, or other owned data.
Constructing a variant moves an affine payload into it; appending moves that
wrapper into the list. The loop borrows the list, and matching `self` borrows
the active payload, so neither operation needs a copy.

Each kind has its own implementation of `label`, but the wrapper's method is
what lets the loop call one interface. No protocol declaration is necessary.
Adding a variant makes the compiler check that the wrapper's match handles it.

## Use an anonymous sum for a local choice

If you do not need a named wrapper or its methods, declare the alternatives in
the list's element type. The annotation lets `append` accept either member
directly:

```plenty
class Message:
    text: str

enum Status:
    Ready
    Busy

def main() -> Result[(), AllocError | IoError]:
    mut items: list[Message | Status] = []?
    items.append(Message("hello"))?
    items.append(Status.Ready)?
    for item in &items:
        match item:
            case message: Message:
                print(message.text)?
            case status: Status:
                match status:
                    case Status.Ready:
                        print("ready")?
                    case Status.Busy:
                        print("busy")?
    Ok(())
```
```output
hello
ready
```

`case message: Message:` selects a whole member type. Because `item` is a
borrow, its matched payload is borrowed too. The `Status` member contains any
of that enum's variants, so its own match handles both.

The sum does not forward a method just because every member has one with the
same signature. Match to select a member, or choose the wrapper pattern when
several callers need the same operation. When a protocol describes the shared
method, the sum satisfies it; see
[protocols on anonymous sums](../../design/language/07-generics/protocols.md#anonymous-sums).

Both forms store a tag and payload inline; wrapping adds no heap allocation.
List creation and growth still return allocation results, as does constructing
any payload that itself needs storage. See [anonymous sums](../../design/language/03-types/anonymous-sums.md)
and [enums](../../design/language/03-types/enums.md) for the full contracts.

For implementations supplied independently of a fixed list of alternatives, see
[protocol objects](../09-generics/boxed-protocols.md). Each object uses an explicit
heap allocation and exposes the methods of one protocol.
