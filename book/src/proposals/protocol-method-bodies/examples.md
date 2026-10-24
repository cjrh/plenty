# Short programs with final protocol methods

**Hypothetical Plenty.** These complete sketches illustrate the
[agreed core rules](final-and-visibility.md); the current compiler does not accept
them. The research harness runs handwritten current-language equivalents of
their algorithms, not the new dispatch or conformance rules.

The examples keep hooks public to isolate implementation selection. The
private-hook variation at the end adds the separate cross-module access question.

## Saving a document: required hook, final operation

The protocol author supplies validation once:

```text
enum SaveError:
    Empty

protocol DocumentStorage:
    pub def write(self: &mut DocumentStorage, document: &str) -> Result[(), SaveError]:
        pass

    @final
    pub def save(self: &mut DocumentStorage, document: &str) -> Result[(), SaveError]:
        if document.byte_len() == 0:
            return Err(SaveError.Empty)
        self.write(document)?
        Ok(())

class Memory:
    writes: i64

    pub def write(self: &mut Memory, document: &str) -> Result[(), SaveError]:
        self.writes += 1
        Ok(())

def persist[S: DocumentStorage](storage: &mut S, document: &str) -> Result[(), SaveError]:
    storage.save(document)

def main() -> Result[(), IoError]:
    mut memory = Memory(0)
    match persist(&mut memory, ""):
        case Ok(_):
            print("unexpected")?
        case Err(_):
            print("empty rejected")?
    match persist(&mut memory, "hello"):
        case Ok(_):
            print("saved")?
        case Err(_):
            print("unexpected")?
    print(memory.writes)?
    Ok(())
```

Expected output: `empty rejected`, `saved`, `1`. The class author writes
only storage behavior. The application uses `persist`, which explicitly names
the contract and can call its final `save`.

If `Memory` adds its own `save`, the **call to `persist`** is rejected, even if
that inherent method validates correctly. It is the attempted structural match,
not the class declaration, that violates this candidate's contract.

`DocumentStorage.save(&mut memory, "hello")` would be the qualified spelling.
`memory.save("hello")` does not gain a new inherent method. Calling
`memory.write("hello")` remains possible in this public-hook example.

## Scoring: final operation calls a replaceable default

```text
protocol Scored:
    pub def base_score(self) -> i64:
        pass

    pub def score(self) -> i64:
        self.base_score()

    @final
    pub def eligible(self) -> bool:
        self.score() >= 10

class Basic:
    points: i64

    pub def base_score(self) -> i64:
        self.points

class Bonus:
    points: i64

    pub def base_score(self) -> i64:
        self.points

    pub def score(self) -> i64:
        self.points + 5

def qualifies[S: Scored](item: &S) -> bool:
    item.eligible()

def main() -> Result[(), IoError]:
    basic = Basic(7)
    bonus = Bonus(7)
    print(qualifies(&basic))?
    print(qualifies(&bonus))?
    Ok(())
```

Expected output: `False`, `True`. The final threshold algorithm calls the
selected `score`: the protocol fallback for `Basic`, the override for `Bonus`.
It does not freeze every method reachable from its body.

From the class author's perspective, `Bonus.score` is still an ordinary method.
It may be called directly. `Basic` has no inherent `score`; uniform application
code uses a bound or `Scored.score(&basic)`. This asymmetry is the main ergonomic
cost of avoiding automatic injection, and should be evaluated with users.

## Same object, two protocols, different provided behavior

```text
protocol Celsius:
    pub def celsius(self) -> i64:
        pass

    @final
    pub def display(self) -> i64:
        self.celsius()

protocol Fahrenheit:
    pub def celsius(self) -> i64:
        pass

    @final
    pub def display(self) -> i64:
        self.celsius() * 9 // 5 + 32

class Sensor:
    reading: i64

    pub def celsius(self) -> i64:
        self.reading

def show[T: Celsius, U: Fahrenheit](left: &T, right: &U) -> Result[(), IoError]:
    alias = left
    print(alias.display())?
    print(right.display())?
    Ok(())

def main() -> Result[(), IoError]:
    sensor = Sensor(20)
    show(&sensor, &sensor)?
    Ok(())
```

Expected output: `20`, `68`. Both generic arguments are concretely `Sensor`,
but their bounds select different implementations. The alias must retain the
left parameter's protocol context. Neither default is installed globally on
`Sensor`, and neither protocol overrides the other.

This is why simply registering a synthetic `Sensor.display` when first
encountering a bound would be incorrect. It would make later calls depend on
which protocol was instantiated first.

## Making the storage hook private across modules

This variation is a syntax sketch, not another complete runnable program:

```text
# library.plenty
pub protocol DocumentStorage:
    def write(self: &mut DocumentStorage, document: &str) -> Result[(), IoError]:
        pass

    @final
    pub def save(self: &mut DocumentStorage, document: &str) -> Result[(), IoError]:
        # Validate, call self.write(document), record success.
        ...

# backend.plenty
from library import DocumentStorage

pub class FileStorage:
    @protocol.DocumentStorage  # Agreed access-grant syntax; not implemented.
    def write(self: &mut FileStorage, document: &str) -> Result[(), IoError]:
        # Perform the actual file operation.
        ...

# application.plenty
def persist[S: DocumentStorage](storage: &mut S, document: &str) -> Result[(), IoError]:
    storage.save(document)   # Allowed.
    # storage.write(document) would be rejected.
```

Here the class author explicitly permits the real library's protocol to call
`write`; the application cannot create an equivalent protocol to acquire that
permission. The compiler checks the annotation where `FileStorage` is defined:
`DocumentStorage` must declare a non-public `write` with the same signature. No separate implementation block is required. Ordinary module
privacy still lets code inside `backend.plenty` call its own hook. All methods
defined in `DocumentStorage` may use the grant, but standalone helpers in
`library.plenty` do not inherit it. Class-only privacy was not selected.

The imports run one way: `backend.plenty` imports `library.plenty`, and the
application imports both. `library.plenty` never names `FileStorage`, because
protocol bodies are checked against the protocol's own contract.
