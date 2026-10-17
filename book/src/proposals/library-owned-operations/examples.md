# Transport wrapper and capability examples

**Status: Executable design example; not a stdlib API.** These experiments use
existing Plenty, supporting the [assessment](../library-owned-operations.md).
They do not add stdlib types or change the language.

## Baseline: three authors, three modules

Save the following files beside each other and run `plentyc main.plenty`.
The library author owns `library.plenty`; the backend author owns
`backend.plenty`; the application author owns `main.plenty`. `Memory` stands in
for an external backend with success and failure outcomes. A TCP backend would
need the partial-progress/error contract described in the assessment.

`library.plenty`:

```text
pub enum TransportError:
    Closed
    TooLarge
    Backend

pub protocol Backend:
    def send_impl(self: &mut Backend, message: &str) -> Result[(), TransportError]:
        pass

pub class Transport[B]:
    _backend: B
    _limit: i64
    _open: bool
    _sent: i64

    pub def send(self: &mut Transport[B], message: &str) -> Result[(), TransportError]:
        if not self._open:
            return Err(TransportError.Closed)
        if message.byte_len() > self._limit:
            return Err(TransportError.TooLarge)
        match deliver(&mut self._backend, message):
            case Ok(_):
                pass
            case Err(cause):
                self._open = False
                return Err(cause)
        self._sent += 1
        Ok(())

    pub def sent(self) -> i64:
        self._sent

    pub def close(self: &mut Transport[B]) -> ():
        self._open = False

def deliver[B: Backend](backend: &mut B, message: &str) -> Result[(), TransportError]:
    backend.send_impl(message)

pub def transport[B: Backend](backend: B, limit: i64) -> Transport[B]:
    Transport(backend, limit, True, 0)
```

`backend.plenty`:

```text
from library import TransportError

pub class Memory:
    pub def send_impl(self: &mut Memory, message: &str) -> Result[(), TransportError]:
        if message == "fail":
            return Err(TransportError.Backend)
        Ok(())
```

`main.plenty`:

```text
from library import transport
from backend import Memory

def main() -> Result[(), Failure]:
    mut t = transport(Memory(), 4)
    print(str.repr(t.send("ok"))?)?
    print(str.repr(t.send("large"))?)?
    print(str.repr(t.send("fail"))?)?
    print(t.sent())?
    t.close()
    print(str.repr(t.send("ok"))?)?
    Ok(())
```

The calls produce success, `TooLarge`, `Backend`, count `1`, and `Closed`.
The application deliberately uses `str.repr` to show Result wrappers. The
rejected oversized message never reaches the hook; a backend error poisons the
transport and never increments the count. `close` is also an explicit operation.

`transport[B: Backend]` validates every requirement when constructed. The
private `deliver[B: Backend]` helper supplies the bound for delegation from
an otherwise unconstrained generic class method. This is existing Plenty;
`class Transport[B: Backend]` is not available today.

An application cannot call `t.send_impl` (no such method) or `t._backend`
(private field). It *can* construct a separate `Memory` and call its public
`send_impl` directly. That separate value is not the owner moved into `t`, but
separate handles could still alias the same external resource. Private wrapper
fields do not establish exclusive access to the underlying socket or database.

## Capability variant: authorize calls as well as implementations

Keep all three files above. In `library.plenty`, add this type before the
protocol:

```text
pub class Permit:
    _issued: bool

    def __del__(self: &mut Permit) -> ():
        pass
```

The generated field constructor is private because `_issued` is private.
The destructor is deliberately empty: custom destruction disables structural
`copy`. It is an awkward copy barrier, not cleanup logic or a proposed language
idiom. The flag's value is irrelevant; nominal type identity and private
construction carry the authority.

Replace the protocol and private helper on the same page with:

```text
pub protocol Backend:
    def send_impl(self: &mut Backend, message: &str, permit: &Permit) -> Result[(), TransportError]:
        pass

def deliver[B: Backend](backend: &mut B, message: &str) -> Result[(), TransportError]:
    permit = Permit(True)
    backend.send_impl(message, &permit)
```

In `backend.plenty`, import `Permit` alongside `TransportError` and add
`permit: &Permit` after `message: &str` in `Memory.send_impl`. Its body can stay
unchanged. The application file stays unchanged, and produces the same outcomes.
All companion code for these replacements is the baseline on this page.

The external backend author can name the public type without constructing it.
Ordinary application calls without a permit fail arity checking; both
`Permit(True)` and `Permit.new(True)` fail private-constructor checking.
Attempting `saved = copy(permit).unwrap()` in the backend fails with
`this resource cannot be copied`. Removing the destructor makes that explicit
copy legal, despite the private flag. This matters if the backend should be
unable to retain an issued capability accidentally.

A hook can still invoke another hook with the borrowed permit during its call.
The example does not prove exactly-once invocation, receiver/message identity,
transaction sequencing, or correct external effects. Application code that is
explicitly handed a permit also gains call authority. The library must choose
which functions receive it.

## Probe results

The compiler was freshly built from `5346f85` with
`cargo build --locked --offline --bin plentyc` in the issue worktree, using a
separate target directory. Both variants compiled and ran natively. These were
focused manual probes, not additions to the tutorial's executed-fence suite.
Proposal fences intentionally use `text`.

| Change to the example | Observed result |
| --- | --- |
| Call `t.send_impl("ok")` | Rejected: unknown method on `Transport` |
| Access `t._backend` | Rejected: private member |
| Make `Memory.send_impl` private | Rejected at construction requiring the protocol; a matching name does not grant access |
| Make `Memory` private | Rejected importing the class; a public method does not export its class |
| Call a separately constructed baseline backend directly | Accepted |
| Omit capability argument on a separate backend | Rejected: hook expects two arguments |
| Construct capability through either constructor spelling | Rejected: private `Permit.__new__` |
| Explicitly copy borrowed capability | Rejected with custom destruction; accepted without it |

The source also confirms lexical access in
`src/frontend/protocols.rs` (`check` calls `check_member_at`) and in protocol
inference in `src/frontend/generics.rs`. Existing `tests/test_modules.rs` covers
private classes/methods, transitive generic calls and definition-module access.
The generic caller does not lend its access to the library's helper.
