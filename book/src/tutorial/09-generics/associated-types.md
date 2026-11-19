# Use associated types in generic code

Use an associated type when selecting an implementation already determines
a type your generic code needs. The methods give the compiler the evidence.
For example, `R.Reading` names the type returned by the particular reader
`R`. The implementations just define their methods.

## Take a device reading

A monitoring tool reads a temperature as a number and a door contact as a
boolean. The examples use stored readings in place of live hardware.

```plenty
protocol Reader:
    type Reading

    def read(self) -> Reading:
        pass

class Thermometer:
    celsius: f64

    def read(self) -> f64:
        self.celsius

class DoorContact:
    open: bool

    def read(self) -> bool:
        self.open

def snapshot[R: Reader](device: &R) -> R.Reading:
    device.read()

def main() -> Result[(), IoError]:
    thermometer = Thermometer(21.5)
    door = DoorContact(True)
    temperature: f64 = snapshot(&thermometer)
    door_open: bool = snapshot(&door)
    print("Celsius:", temperature)?
    print("Door open:", door_open)?
    Ok(())
```
```output
Celsius: 21.5
Door open: True
```

A parameterized `Reader[T]` can express the same contract with
`snapshot[T, R: Reader[T]]`. Here the device's signature already fixes
`T`, so `R.Reading` lets the function name it directly. The calling code
gets the exact type of the reading and can use it immediately.

## Parse configuration values

A configuration loader receives text. A retry count needs an integer parser,
while a setting such as `logging=on` needs a switch parser. Each parser
also has its own failure type.

```plenty
protocol SettingParser:
    type Parsed
    type ParseFailure

    def parse(self, text: &strview) -> Result[Parsed, ParseFailure]:
        pass

class RetryCountParser:
    def parse(self, text: &strview) -> Result[i64, ParseError]:
        i64.parse(text)

enum SwitchError:
    ExpectedOnOrOff

class SwitchParser:
    def parse(self, text: &strview) -> Result[bool, SwitchError]:
        if text == "on":
            return Ok(True)
        if text == "off":
            return Ok(False)
        Err(SwitchError.ExpectedOnOrOff)

def load_setting[P: SettingParser](parser: &P, text: &strview) -> Result[P.Parsed, P.ParseFailure]:
    parser.parse(text)

def main() -> Result[(), ParseError | SwitchError | IoError]:
    retries = load_setting(&RetryCountParser(), "3")?
    logging = load_setting(&SwitchParser(), "on")?
    print("Retries:", retries)?
    print("Logging:", logging)?
    Ok(())
```
```output
Retries: 3
Logging: True
```

A single shared error enum would make the generic loader translate failures
or commit to a list of supported parsers. Projecting `P.ParseFailure` keeps each
parser's full error available to its caller. This application combines the
two parser error types with `IoError` in `main` and propagates failures
with `?`.

## Look up a record by its key

A stock report attaches counts to catalog records. The record supplies its
own key, and the report's dictionary must accept that key type.

```plenty
protocol CatalogEntry:
    type Key: KeyType

    def stock_key(self) -> Key:
        pass

class Part:
    sku: i64

    def stock_key(self) -> i64:
        self.sku

def stock_for[E: CatalogEntry](entry: &E, counts: &dict[E.Key, i64]) -> Option[i64]:
    counts.get(entry.stock_key())

def main() -> Result[(), AllocError | IoError]:
    counts = {1042: 18}?
    match stock_for(&Part(1042), &counts):
        case Some(quantity):
            print("In stock:", quantity)?
        case Nothing:
            print("No stock record")?
    Ok(())
```
```output
In stock: 18
```

A dictionary-specific helper could require every catalog to use integer
SKUs. With `E.Key`, another catalog can use a different supported key
type and the same lookup. `KeyType` guarantees that the projected type
is usable as a dictionary key. The signature ties the dictionary's key to the
entry's key, so passing a dictionary for a different key type fails at
the call. The `Option` preserves the distinction between a missing record
and a quantity of zero.

## Deliver an alert to a compatible destination

An alert panel produces a message, and a console knows how to display one.
A delivery function can accept independently implemented sources and
destinations as long as they agree on the message type. The panel in this
example returns a fixed sample alert.

```plenty
class Alert:
    room: str
    message: str

protocol AlertSource:
    type Message

    def pending(self) -> Option[Message]:
        pass

protocol AlertDestination:
    type Input

    def send(self, message: Input) -> Result[(), IoError]:
        pass

class AlarmPanel:
    def pending(self) -> Option[Alert]:
        Some(Alert("server room", "high temperature"))

class Console:
    def send(self, message: Alert) -> Result[(), IoError]:
        print(message.room, message.message)

def deliver[S: AlertSource, D: AlertDestination[Input = S.Message]](
    source: &S, destination: &D
) -> Result[(), IoError]:
    match source.pending():
        case Some(message):
            destination.send(message)?
        case Nothing:
            pass
    Ok(())

def main() -> Result[(), IoError]:
    deliver(&AlarmPanel(), &Console())
```
```output
server room high temperature
```

Two parameterized protocols could share an extra type parameter in
`deliver`. Here `Input = S.Message` states the compatibility rule directly:
the panel produces an `Alert`, so the destination must accept an `Alert`.
A destination that accepts `str` would need an explicit formatting adapter
before it could take part in this delivery.

Use a [parameterized protocol](generic-protocols.md) when the API should
express a type as a separate input to the bound. Use an associated type
when the selected implementation already determines it. See the
[associated-type reference](../../design/language/07-generics/protocols.md#associated-types)
for inference rules, bounds, and equality constraints.
