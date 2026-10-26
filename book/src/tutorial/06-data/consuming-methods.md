# Consume a value to change its state

Use `self: Class` when an operation finishes an object or replaces it with a
different state. The method receives ownership, so the caller cannot use the
old class instance again. Bare `self` still borrows; `self: &mut Class` mutates
an instance that remains available to the caller.

Here a draft must be validated before it can be finished. Separate classes make
invalid operation ordering a type error. Keeping `Ready`'s field and constructor
private prevents callers from skipping validation:

```plenty-file workflow.plenty
pub class Draft:
    pub text: str

    pub def validate(self: Draft) -> Result[Ready, Draft]:
        if self.text == "":
            return Err(self)
        Ok(Ready(self.text))

pub class Ready:
    text: str

    pub def finish(self: Ready) -> str:
        self.text
```

```plenty
import workflow

def main() -> Result[(), IoError]:
    draft = workflow.Draft("report")
    ready = draft.validate().unwrap()
    print(ready.finish())?
    # An error can explicitly return the draft for correction and retry.
    match workflow.Draft("").validate():
        case Ok(ready):
            print(ready.finish())?
        case Err(retry):
            mut corrected = retry
            corrected.text = "corrected"
            print(corrected.validate().unwrap().finish())?
    Ok(())
```
```output
report
corrected
```

The `unwrap` calls assert that the nonempty strings in this example pass
validation. For input that might fail, recover the owner using `match`, as the
empty-draft example does. A fallible method does not automatically restore the
receiver after failure. The same rule applies to a simple finalization method:

```plenty-error
class Message:
    text: str

    def finish(self: Message) -> str:
        self.text

def main() -> ():
    message = Message("report")
    text = message.finish()
    print(message.text).unwrap()
    drop(text)
```
```error
use of moved binding `message`
```

Ownership moves into the method before its other arguments are evaluated.
Read any needed fields into locals before making the consuming call. If a later
argument returns early with `?`, the pending receiver is cleaned up. Inside the
method, owned `self` is an immutable binding; move it into a `mut` local if you
need to change fields.

This example reads a copyable string field. Owned list or resource fields still
cannot be moved directly out of a class. Use existing enum-payload moves or
`Option` plus [`replace`](../../design/language/04-expressions/assignment.md#replacing-a-field)
when a transition needs to transfer such storage.

Consuming a value does not require callers to finish it: they can still drop it.
Normal cleanup runs exactly once unless the method transfers ownership onward.
Enums also accept by-value receivers, but a copyable enum is copied according to
ordinary argument rules. Use affine state types when the old state must cease
to be usable. See [method contracts](../../design/language/06-functions/01-signatures-and-calls.md).
