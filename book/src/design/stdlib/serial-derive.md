# Derived serialization

`@serial` on a class or enum generates private `__encode__` and `__decode__`
hooks for [`std.serial`](serial.md). Each explicitly written hook replaces that
side of derivation. Generated hooks use the ordinary type checker and static
codec dispatch, including private handwritten hooks on nested values. No runtime
reflection or retained field descriptor is involved. The annotation imports its
compiler dependency under an inaccessible name; the module need not import
`std.serial` unless it names that module itself.

## Classes

All declared fields, including private fields, are encoded in declaration order
under their source names, or under their [wire names](#wire-names). Decoding accepts fields in any order, rejects
duplicates and missing required fields, and delegates unknown fields to the
format adapter's `unknown_fields` policy. An absent `Option[T]` becomes `Nothing`.
A present `null` also becomes `Nothing`; its field slot remains present, so a
second occurrence is still a duplicate. Type aliases and generic field types
are checked through their actual specialized types.

```plenty
import std.json

@serial
class Endpoint:
    host: str
    port: Option[u16]

def main() -> Result[(), IoError]:
    options = std.json.codec_options()
    endpoint = std.json.load[Endpoint](b"{\"host\":\"localhost\"}", &options).unwrap()
    print(endpoint.port)?
    Ok(())
```

```output
Option[u16].Nothing
```

JSON writes every field, including `Nothing` as `null`. TOML omits `Nothing`
class fields because it has no null value. This omission applies to class fields;
it does not silently remove dictionary entries or array elements. If an optional
field has a constructor default of `Some(...)`, reloading an omitted `Nothing`
uses that default; see [TOML representation limits](toml-codecs.md#representation).

A class without `__init__` uses its normal generated field constructor. An
explicit initializer must accept exactly the fields by name and the same semantic type
for automatic decoding. For generic classes, field and error-domain eligibility
uses the concrete
type arguments. The generated hook calls the real initializer, with
named arguments in its parameter declaration order, so validation is preserved.
An initializer whose parameter names do not match the fields receives only an
encoding hook. Write `__decode__` when construction needs a different shape.

Initializer parameter defaults make their fields optional. Defaults are evaluated
only for absent members, after decoding, in initializer parameter declaration
order. An explicit value, including `Nothing`, skips the default. Defaults resolve
names in the defining module and produce fresh owners under the normal function
rules.

Fallible defaults and initializer bodies share the initializer's error domain.
Automatic decoding accepts `std.serial.Fault`, `AllocError`, or exactly
`AllocError | std.serial.Fault`, written directly or through an alias. A `Fault`
is mapped through the adapter's `reject` method and an `AllocError` through its
`allocation` method, so a failed `list[str] = []?` default reports the format's
allocation fault, never `Failure` or an abort. A failing default keeps its
field path; an initializer body failure has no path. Other construction error
domains, including sums with further members, require a handwritten decoding
hook to choose a mapping. A domain that uses a class type parameter, as in
`Result[(), E]` or `Result[(), AllocError | E]`, is checked and mapped for each
specialization: `Config[AllocError]` decodes, while `Config[IoError]` requires a
handwritten hook.
Encoding remains independent of construction.

Slots are grouped in a typed tuple rather than requiring a temporary chain per
field. Derivation supports records with 64 fields under the normal compiler's
local-variable limit. Ordinary compiler nesting and specialization limits still
apply; no separate unlimited record size is promised.

## Enums

A nullary variant is encoded as its name string. A variant with payload is an
object containing exactly one member whose name is the variant name. Either
name is the variant's [wire name](#wire-names) when it has one. A single
payload is encoded directly; two or more payloads use a fixed-length array.
For example, `Mode.Idle`, `Mode.Value(3)` and `Mode.Pair("x", 4u8)` become
`"Idle"`, `{"Value":3}` and `{"Pair":["x",4]}` in JSON.

Decoding rejects unknown variants, a payload on a nullary variant, a missing
payload, objects with zero or multiple tags, and payload arrays with the wrong
arity. A payload failure carries `Step.Variant`; positional payload failures also
carry `Step.Index`. Generic enums specialize their payload codecs normally.

## Wire names

`@serial.name("wire")`, alone on the line above a field or variant, sets the
name that member is encoded and decoded under. It applies to class object keys
and to enum tags, with or without payload, in every format.

```plenty
import std.json

@serial
enum AgentKind:
    @serial.name("pi")
    Pi
    @serial.name("command")
    Command(str)

@serial
class Agent:
    @serial.name("agent-kind")
    kind: AgentKind
    name: str

def main() -> Result[(), IoError]:
    options = std.json.codec_options()
    agent = std.json.load[Agent](b"{\"agent-kind\":{\"command\":\"run\"},\"name\":\"a\"}", &options).unwrap()
    print(agent.kind)?
    print(std.json.dump(&Agent(AgentKind.Pi, "b"), &std.json.limits()).unwrap().to_text().unwrap())?
    Ok(())
```

```output
AgentKind.Command("run")
{"agent-kind":"pi","name":"b"}
```

The wire name replaces the source name; it is not an alias. Decoding rejects
the source name as an unknown field or variant, under the usual
`unknown_fields` policy. Missing-field and duplicate-field faults and
`Step.Field`/`Step.Variant` path steps report wire names, because they locate
data in the document. The source name remains the member's name everywhere
else: field access, privacy, `match` patterns, printing, and the constructor
parameter that decoding passes the value to.

The compiler rejects:

- `@serial.name` on a method, at top level, or in a type without `@serial`;
- an argument other than one nonempty string literal, and a second
  `@serial.name` on the same member;
- a wire name equal to the member's source name;
- two members of one type with the same effective wire name, including a
  renamed member and an unchanged one;
- `@serial.name` in a type that writes both `__encode__` and `__decode__`,
  where it would have no effect.

A type that writes one hook keeps the wire name on its derived side; the
handwritten hook must use the same name. There is one wire name per member;
case-conversion policies, decode aliases and per-format names are not
supported.

## Composition and errors

Derived and handwritten hooks compose through classes, enums, collections,
`Box` and recursive types. Format nesting limits and the
[native stack budget](serial.md#errors-and-costs) stop recursive decoding and
encoding with typed errors. The generated hooks use no worker-ineligible operation, so ordinary
worker eligibility follows the concrete fields, hooks and adapter.

Failures retain the format's typed cause inside `SerialError`. Field and variant
steps are added lazily on failure; successful operations do not allocate path
entries. Failure while allocating a path keeps the original cause and marks the
path incomplete. Constructor/default failure drops already decoded owned fields
and never constructs an invalid partial instance.

The annotation removes application boilerplate; it retains the compile-time,
object-size and runtime costs of ordinary specialized hooks. See the
[serialization measurements](../../proposals/completed/derived-serialization.md#measurements)
for the measurement contract.
