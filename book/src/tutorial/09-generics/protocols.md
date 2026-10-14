# Requiring methods with a protocol

A protocol names the methods a generic algorithm needs. A class satisfies it
by having matching methods; no inheritance or registration is required.
Importing a protocol does not add methods to a class.

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

Here `&message` determines `T = Message`; the compiler then checks the `Readable`
requirements. Writing `read_message[Message](&message)` explicitly also works.

`self` means a shared receiver. Write `self: &mut ProtocolName` when a required
method mutates the receiver. Method parameters, return types, and receiver
borrowing must match exactly. All required methods are checked at specialization,
including methods the generic function does not happen to use. Across modules,
the class methods must be visible to the generic function's defining module.
That means both `pub class Message` and `pub def read`, even when the caller is
in `Message`'s own module. The caller does not lend private access to the helper.
If only the class is private, the call diagnostic asks you to declare it
`pub class`; private methods still need their own `pub def`.

For example, save this helper as `reader.plenty`:

```plenty-file reader.plenty
pub protocol Readable:
    def read(self) -> str:
        pass

pub def read_message[T: Readable](source: &T) -> str:
    source.read()
```

This application needs `pub class Message`, as well as its public method:

```plenty-error
import reader

class Message:
    pub def read(self) -> str:
        "hello"

def main() -> ():
    message = Message()
    drop(reader.read_message(&message))
```
```error
class `Message` is private to its defining module; declare it `pub class`
```

Protocols currently constrain class type arguments; they cannot be stored as
values. Use `source: &T` with `T: Readable`, not `source: Readable`.
