# Dictionaries with different kinds of values

A dictionary has one key type and one value type. The value type can be an enum
wrapper or an anonymous sum, so a string key can select any of several kinds of
stored data. Keys retain the narrower [key-type contract](../../design/facilities/01-builtins/collections.md):
integers, `bool`, `str`, `bytes`, or tuples, classes, enums and anonymous sums
recursively containing those types. Keys with custom cleanup can be borrowed through `keys()` or
transferred into an owned list with `into_keys()`; they cannot be copied with
`keys().to_set()`. See [cleanup-bearing keys](sets.md#keys-with-custom-cleanup).

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

An anonymous sum can combine supported key types without merging their identities:

```plenty
def main() -> Result[(), AllocError | IoError]:
    labels: dict[i64 | str, str] = {7: "numeric account", "7": "named account"}?
    print(labels[7], labels["7"])?
    Ok(())
```
```output
numeric account named account
```

A named enum can represent these distinct key domains while preserving their
identity:

```plenty
enum Key:
    Id(i64)
    Name(str)

def main() -> Result[(), AllocError | IoError]:
    labels: dict[Key, str] = {
        Key.Id(7): "numeric account",
        Key.Name("7"): "named account",
    }?
    print(labels[Key.Id(7)], labels[Key.Name("7")])?
    Ok(())
```
```output
numeric account named account
```

Every enum payload must itself be a supported key type. You can also keep separate
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

Keys and values can independently use sums. Only the key alternatives must
satisfy the key-type contract:

```plenty
def main() -> Result[(), AllocError | IoError]:
    mut settings: dict[i64 | str, i64 | str] = {7: "paused", "retries": 3}?
    settings.insert("mode", "careful")?
    print(settings[7], settings["retries"], settings["mode"])?
    Ok(())
```
```output
"paused" 3 "careful"
```

The annotations provide the expected types for insertion and lookup. The
integer key `7` remains distinct from the string key `"7"`. Use a named enum
when domain-specific variant names make the meaning clearer.

For implementations supplied independently of a fixed list of alternatives, see
[protocol objects](../09-generics/boxed-protocols.md). Each object uses an explicit
heap allocation and exposes the methods of one protocol.
