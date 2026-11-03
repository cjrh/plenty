# Dictionaries with different kinds of values

A dictionary has one key type and one value type. The value type can be an enum
wrapper or an anonymous sum, so a string key can select any of several kinds of
stored data. Keys retain the narrower [key-type contract](../../design/facilities/01-builtins/collections.md):
integers, `bool`, `str`, `bytes`, or tuples recursively containing those types.

## Give mixed values a named wrapper

An enum makes the kinds of settings explicit. Here each value carries either
a retry policy class or a mode enum:

```plenty
class RetryPolicy:
    attempts: i64

enum Mode:
    Fast
    Careful

enum Setting:
    Retries(RetryPolicy)
    Mode(Mode)

def main() -> Result[(), AllocError | IoError]:
    mut settings: dict[str, Setting] = {}?
    settings.insert("network", Setting.Retries(RetryPolicy(3)))?
    settings.insert("processing", Setting.Mode(Mode.Careful))?
    for name, setting in settings.items():
        match setting:
            case Setting.Retries(policy):
                print(*name, policy.attempts)?
            case Setting.Mode(mode):
                print(*name, *mode)?
    Ok(())
```
```output
network 3
processing Mode.Careful
```

`insert` adds or replaces an entry and handles possible allocation with `?`.
The dictionary owns each wrapper and its payload. `items()` borrows them in
insertion order, so inspecting a class payload does not copy or remove it.
A wrapper can also provide shared methods, as in the [list example](lists.md#give-the-alternatives-a-shared-interface).

For a lookup that may miss, `get` requires a copyable value: this `Setting`
contains an affine class, so `get` cannot return it. Use `get_mut` to borrow an
optional value for an update, or `pop` to remove and take ownership of it.
See [dictionary lookup and removal](../../design/facilities/01-builtins/dictionaries.md#lookup-and-removal).

## Use a sum for simple values

An anonymous sum avoids declaring variants when the whole types express the
choice. The annotation supplies the expected type for both a literal and later
insertions:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut settings: dict[str, i64 | str] = {"retries": 3}?
    settings.insert("mode", "careful")?
    for name, value in settings.items():
        match value:
            case count: i64:
                print(*name, *count)?
            case text: str:
                print(*name, *text)?
    Ok(())
```
```output
retries 3
mode careful
```

The sum admits only the listed types. If two alternatives have the same payload
type but different meanings, such as seconds and attempts both stored as `i64`,
use separate named enum variants instead: `i64 | i64` is just `i64`.

## Different kinds of keys

A sum is not a supported dictionary key, even when its members are individually
supported:

```plenty-error
def main() -> Result[(), AllocError]:
    labels: dict[i64 | str, str] = {}?
    Ok(())
```
```error
dictionary keys must be
```

Wrapping those keys in an enum does not change that restriction. Keep separate
dictionaries when numeric IDs and textual names are different lookup domains:

```plenty
def main() -> Result[(), AllocError | IoError]:
    by_id: dict[i64, str] = {7: "numeric account"}?
    by_name: dict[str, str] = {"7": "named account"}?
    print(by_id.get(7).unwrap(), by_name.get("7").unwrap())?
    Ok(())
```
```output
numeric account named account
```

These lookups unwrap because the example has just inserted both keys. For
external input, match the `Option` or supply an intentional default instead.
Do not merge distinct identities by converting every key to a string.

## Different kinds on both sides

`dict[i64 | str, i64 | str]` is rejected for its key type too. You can keep the
key domains separate while sharing a mixed value type:

```plenty
enum Value:
    Count(i64)
    Text(str)

def main() -> Result[(), AllocError | IoError]:
    mut by_id: dict[i64, Value] = {}?
    mut by_name: dict[str, Value] = {}?
    by_id.insert(7, Value.Count(3))?
    by_id.insert(8, Value.Text("paused"))?
    by_name.insert("editors", Value.Count(2))?
    by_name.insert("guests", Value.Text("read only"))?
    print(by_id.get(7).unwrap(), by_name.get("guests").unwrap())?
    Ok(())
```
```output
Value.Count(3) Value.Text("read only")
```

`Value` is copyable because both payload types copy, so `get` works here.
Each dictionary accepts every `Value` variant. A class can group the two
dictionaries when callers need to pass them around as one registry.
