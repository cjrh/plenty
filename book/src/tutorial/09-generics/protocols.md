# Requiring methods with a protocol

A reusable algorithm may need to call a method on its input, rather than
just move the input through unchanged. A protocol describes those required
methods. A class satisfies
it by having methods with matching signatures; no inheritance or registration
is required. Importing a protocol does not add methods to a class.

```plenty
protocol Readable:
    def read(self) -> str:
        pass

class Message:
    text: str
    def read(self) -> str:
        self.text

def read_message[T: Readable](source: &T) -> str:
    source.read()

def main() -> Result[(), Failure]:
    message = Message("hello")
    print(read_message(&message))?
    Ok(())
```
```output
hello
```

`self` means a shared receiver. Write `self: &mut ProtocolName` when a required
method mutates the receiver. Method parameters, return types, and receiver
borrowing must match exactly. All required methods are checked at specialization,
including methods the generic function does not happen to use. Across modules,
the class methods must be visible to the generic function's defining module.

Here `&message` determines `T = Message`; the compiler then checks the `Readable`
requirements. Writing `read_message[Message](&message)` explicitly also works.
The protocol does not make the compiler search for a type to use.

Protocols currently constrain class type arguments; they cannot be stored as
values. Use `source: &T` with `T: Readable`, not `source: Readable`. Protocol
fields, inheritance, and default method implementations are deferred.
