# Derived serialization for classes and enums

**Status: Not implemented.** Nothing shipped; this page is the design record
for issue #471. [`std.json`](../design/stdlib/json.md) and
[`std.toml`](../design/stdlib/toml.md) remain authoritative: neither derives
codecs, and a typed codec is written by hand over JSON events or a TOML tree.
No backlog entry exists yet; [B51](../backlog.md) lists derived typed codecs as
a candidate under B38. The design questions are decided; see
[Decisions](#decisions).

## Problem

Reading a configuration file into a class means writing a codec by hand:

```text
pub class Server:
    pub host: str
    pub port: u16
    pub tags: list[str]
    pub timeout: Option[i64]
```

A JSON reader for `Server` drives `json.Decoder.next()`, matches every
`Event`, accepts members in any order, rejects duplicates and unknown members,
reports which members are missing, checks that the port fits `u16`, and loops
over the tag array. A TOML reader repeats the work over `Toml.Table` with
`find`. Writers repeat it again in both directions. The code is long, easy to
get subtly wrong (a forgotten duplicate check, a missing range check), and has
to change with every field.

Rust solves this with `serde` derives; Python with runtime reflection
(dataclasses, pydantic). Plenty has neither user macros nor runtime
reflection, and its goals rule out hidden allocation and slow compilation.
This page chooses how the per-type code is produced, how a type opts in, how
each kind of type maps to formats, and how errors report where they happened.

## What exists today

Probes in this section ran on local `main` at `7923a7c0` with a debug compiler.

| Fact | Consequence for a design |
| --- | --- |
| A method may declare its own type parameters, including a bound that names another one: `def encode[E, S: Sink[E]](self, out: &mut S) -> Result[(), E]` compiles, both on a class and as a protocol requirement | A format-neutral protocol can carry each format's own error type without associated types |
| Protocols are structural; `std.json.Encoder` satisfied a user protocol with its existing `null`, `integer`, `text`, `key`, `start_object` and `end_object` methods, producing `{"x":3,"label":"a"}` | The JSON encoder needs no adapter if the data model uses its method names |
| Unbounded generic bodies are checked per instance; `std.fmt.display` uses this to call a user method without a named bound | Entry points can accept any type and report an unserializable one at the use site |
| A decode shape written by hand (an `Option` local per field, a `str` `match` on member names, construction after the loop) compiled and decoded members in either order | Derived decoding needs no new runtime machinery |
| Methods require a `self` receiver; a receiver-less `def decode(n: i64) -> Point` is rejected | Decoding, which has no value to call a method on, needs a type-directed hook |
| Protocols constrain only classes, enums and `bytes`; `i64`, `str` and `list[T]` have no methods | A protocol bound cannot admit built-in field types, as the [hashing proposal](user-type-hashing.md#assessing-the-__hash__-protocol-idea) also found |
| Generic code reaches another module's methods only when the type and method are `pub`; automatic destruction ignores `__del__` privacy | Compiler-dispatched hooks can follow the `__del__` rule instead |
| The runtime prints `Point(x=3, y=4)` by walking a static descriptor with field names (`plenty-runtime/src/aggregates.rs`) | A descriptor-driven serializer is possible, at the cost discussed below |
| `std.json` has a resumable event decoder; `std.toml` has only a tree, because TOML tables can be extended anywhere in a file | The data model must work over both an event stream and a tree |
| There is no annotation syntax today; [protocol method bodies](protocol-method-bodies/final-and-visibility.md#method-annotations) agreed a closed set of `@` annotations (`@final`, `@protocol.Name`), not yet implemented | An opt-in can extend that closed set rather than introduce user decorators |

## Precedents

| System | Where per-type code comes from | Formats | Field options |
| --- | --- | --- | --- |
| Rust `serde` | Proc-macro derive generates `Serialize`/`Deserialize` against a 29-type data model; formats implement `Serializer`/`Deserializer` | Any, via one data model | `#[serde(rename, default, skip, flatten, tag)]` |
| Swift `Codable` | Compiler synthesizes `encode(to:)` and `init(from:)` when a type declares conformance; `CodingKeys` enum names fields | Any, via `Encoder`/`Decoder` protocols with keyed containers | Rename by writing `CodingKeys` |
| Kotlin `kotlinx.serialization` | Compiler plugin generates a serializer and a static descriptor per `@Serializable` class | Any, via format-neutral encoders | `@SerialName`, defaults from constructor |
| Zig `std.json` | Comptime field iteration (`inline for (std.meta.fields(T))`) in the format library | Each format walks types itself | Per-type override functions |
| Go `encoding/json` | Runtime reflection over struct metadata | One package per format | Struct tags (`json:"name,omitempty"`) |
| OCaml `ppx_deriving_yojson`, C# `System.Text.Json` source generation | Generated code for one specific format | One deriver per format | Attributes |

Swift and Kotlin are the closest fit: the compiler generates ordinary code
against a format-neutral protocol, and the user can write the same methods by
hand. Serde does the same with a user macro. Zig and Go push the work into each
format library.

## Where the per-type code comes from

| | A. Compile-time field iteration | B. Derived per-type code, format-neutral protocol | C. Generated per-format code | D. Runtime descriptor walk |
| --- | --- | --- | --- | --- |
| Mechanism | A library loop such as `for field in fields[T]()` unrolled at compile time, with type-shape matching | The compiler writes `__encode__`/`__decode__` for an opted-in type, calling a small `std.serial` protocol; each format implements the protocol once | The compiler writes JSON code and TOML code per type | The runtime walks the static type descriptor, as printing does |
| New language | Compile-time loops, field values typed per iteration, type-level `match`, and a partial-construction builder for decoding | One annotation, two hooks, two compiler-expanded std functions | One annotation; format knowledge in the compiler | None visible; a runtime codec per format |
| Adding a format | Write a type walker | Implement one protocol in Plenty | Change the compiler | Change the runtime (Rust) |
| Runtime cost | Direct calls | Direct calls after monomorphization: the JSON instance calls `json.Encoder` methods, as C would | Direct calls | Descriptor interpretation per field, as `==` and printing do |
| Compile time, code size | One unrolled walk per type and format | One body per type and format instance, linear in fields | Same as B | None per type |
| Hand-written override | Per-type escape hatch needed | Write the hook; derived code for outer types calls it | Write per-format code | Needs a descriptor callback slot, as hashing design B |
| Visibility | A library reads every type's fields, including private ones | Code is generated in the type's own module | Same as B | Runtime reads everything |
| Hidden allocation | Depends on the library | None in derived code (see [costs](#costs)) | None | Rust codec allocates internally |
| Errors | Library's choice | One path-carrying wrapper | Per format | Runtime error type |

**A** gives library authors the most power and would make `@serial` itself
library code. It is also the largest language change: decoding needs a way to
build a value field by field from compile-time-typed parts, and a library that
iterates fields bypasses module privacy. It is a general metaprogramming
feature, not a serialization feature; it should be justified on its own.

**C** has no runtime advantage over B. Generic instances are specialized per
concrete type, so B's JSON instance is direct calls on `json.Encoder` with no
dispatch, which is the code C would write. C only moves format knowledge into
the compiler.

**D** costs no compile time but conflicts with the "no runtime reflection" goal,
cannot call a hand-written codec for a nested type without a new callback
slot, and would put format code in Rust instead of checked Plenty.

**Recommendation: B.** The compiler knows one small data model, formats stay
ordinary Plenty, and a derived method is the same code a user could write.

## Recommendation

### Opting in

A type opts in with one closed annotation on its declaration:

```text
@serial
pub class Server:
    pub host: str
    pub port: u16
    pub tags: list[str]
    pub timeout: Option[i64]

@serial
pub enum Mode:
    Off
    Fixed(i64)
    Range(i64, i64)
```

`@serial` extends the closed annotation set agreed for
[protocol method bodies](protocol-method-bodies/final-and-visibility.md#method-annotations)
to class and enum declarations. It derives each of two hooks the type does not
already define:

```text
def __encode__[E, S: Encoder[E]](self, out: &mut S) -> Result[(), SerialError[E]]
def __decode__[E, D: Decoder[E]](source: &mut D) -> Result[Self, SerialError[E]]
```

`Self` stands for the declaring type. `__decode__` is receiver-less, like
`__init__` is a construction hook rather than a method; it is the only
receiver-less hook and does not introduce static methods. A user may write
either hook by hand, with or without `@serial`; derived code for an enclosing
type calls whichever exists. The hooks are invoked only through
`std.serial.encode` and `std.serial.decode`, as `__init__` and `__del__` are
invoked only by construction and destruction, so there is one way to call them.

Without an opt-in, nothing changes: field names do not silently become a wire
format, and renaming a field of an unrelated class never breaks a file format.

### The data model

`std.serial` defines two protocols. The model has JSON's shape, which TOML,
YAML, CBOR and MessagePack also have, and uses `std.json.Encoder`'s method
names so that the JSON encoder satisfies `Encoder[JsonError]` without an
adapter.

```text
pub protocol Encoder[E]:
    def null(self: &mut Encoder) -> Result[(), E]
    def boolean(self: &mut Encoder, value: bool) -> Result[(), E]
    def integer(self: &mut Encoder, value: i64) -> Result[(), E]
    def unsigned(self: &mut Encoder, value: u64) -> Result[(), E]
    def decimal(self: &mut Encoder, value: f64) -> Result[(), E]
    def text(self: &mut Encoder, value: &str) -> Result[(), E]
    def start_array(self: &mut Encoder) -> Result[(), E]
    def end_array(self: &mut Encoder) -> Result[(), E]
    def start_object(self: &mut Encoder) -> Result[(), E]
    def key(self: &mut Encoder, name: &str) -> Result[(), E]
    def end_object(self: &mut Encoder) -> Result[(), E]

pub protocol Decoder[E]:
    def null(self: &mut Decoder) -> Result[bool, E]       # consumes a null if next
    def boolean(self: &mut Decoder) -> Result[bool, E]
    def integer(self: &mut Decoder) -> Result[i64, E]
    def unsigned(self: &mut Decoder) -> Result[u64, E]
    def decimal(self: &mut Decoder) -> Result[f64, E]
    def text(self: &mut Decoder) -> Result[str, E]
    def start_array(self: &mut Decoder) -> Result[(), E]
    def next_element(self: &mut Decoder) -> Result[bool, E]  # False consumes the end
    def start_object(self: &mut Decoder) -> Result[(), E]
    def next_key(self: &mut Decoder) -> Result[Option[str], E]  # Nothing consumes the end
    def unknown_key(self: &mut Decoder, name: &str) -> Result[(), E]
    def reject(self: &Decoder, fault: Fault) -> E
```

The decoder is pull-based and typed: the derived code asks for the type it
expects, so the format parses JSON number text directly into `i64`, `u64` or
`f64`, and TOML returns its stored `Int` or `Float` without a text round trip.
`reject` builds the format's located error from a model-level fault, as
`json.Decoder.reject` does today. `unknown_key` applies the format's policy for
a member the type does not declare: skip its value, or return
`reject(Fault.UnknownField)`. Rejecting is the default; a per-call decode
option skips unknown members instead. The policy is never a per-type setting.

| `Fault` | Raised by derived code when |
| --- | --- |
| `UnexpectedKind` | The next value is not the requested kind |
| `MissingField(str)` | A required field is absent at the end of an object |
| `DuplicateField(str)` | A field appears twice, whatever the format's duplicate-key policy |
| `UnknownField(str)` | Raised by the format's `unknown_key` under the reject policy |
| `UnknownVariant(str)` | An enum tag names no variant |
| `OutOfRange` | A number does not fit the field's type |
| `WrongLength(i64, i64)` | A tuple or variant payload array has the wrong length |
| `DuplicateElement` | A set element or dictionary key repeats |
| `Unrepresentable` | Raised by an encoder for a value its format cannot hold, such as `null` in TOML |

Formats supply implementations in their own modules:

| Format | Encoder | Decoder |
| --- | --- | --- |
| JSON | `json.Encoder` directly | An adapter over `json.Decoder`, fed from a buffer or through `read_event` |
| JSON tree | A builder producing `Json` | A cursor over `&Json` |
| TOML | A builder producing `Toml`, then the existing writer, which orders plain entries before sections | A cursor over `&Toml`, after `parse` |

### How types map

| Plenty type | Model | JSON | TOML |
| --- | --- | --- | --- |
| `bool` | boolean | `true` | `true` |
| `i8`..`i64` | integer; decoding checks the range | number | integer |
| `u8`..`u64` | unsigned; decoding checks the range | number | integer up to `i64.MAX`, else `Unrepresentable` |
| `f32`, `f64` | decimal; an integer is accepted when decoding | number; non-finite is the encoder's existing `NonFiniteNumber` | float, including `inf` and `nan` |
| `str` | text | string | string |
| `Bytes`, `bytes` | text, standard base64 with padding | string | string |
| `list[T]`, `set[T]` | array | array | array, or array of tables |
| `(A, B, ...)` | array of fixed length | array | array |
| `dict[str, V]` | object, insertion order | object | table |
| `Box[T]` | `T` | | |
| Class | object, fields in declaration order | object | table |
| `Option[T]` field | `null` when `Nothing`; absent or `null` decodes as `Nothing` | `null` | omitted |
| `Option[T]` elsewhere | `null` or `T` | `null` | `Unrepresentable` for `Nothing` |
| Nullary variant | its name as text | `"Off"` | `"Off"` |
| Variant with one payload | object with one key | `{"Fixed": 5}` | `{ Fixed = 5 }` |
| Variant with several payloads | object with one key, array value | `{"Range": [1, 9]}` | `{ Range = [1, 9] }` |

Rejected at the use site with the path to the offending field: references
(already excluded from fields), closures, generators, handles and
synchronization types, anonymous sums (their members have no tag names),
`Result`, `Option[Option[T]]` (its two `Nothing` levels would encode alike),
`dict` with non-`str` keys (including integer keys), and any class or enum
with neither `@serial` nor hand-written hooks:

```text
error: `Config` cannot be decoded
  note: field `Config.server` has type `Server`, which has no `__decode__`
  help: add `@serial` to `Server`, or write `__decode__`
```

Recursive types through `Box`, lists or dictionaries are accepted, unlike
automatic `==` and printing: the derived code recurses explicitly, and every
format's `start_array` and `start_object` enforce its `max_depth` in both
directions, so a hostile document or a deep value built by hand stops
at the limit instead of overflowing the stack.

Generic types derive per instance: `Page[T]` with `items: list[T]` is
serializable exactly when `T` is, checked where it is used, with the usual
instantiation note.

### Field names and optional fields

Member names are the declared field names, unchanged. Encoding writes fields in
declaration order; decoding accepts any order. Missing required fields,
duplicates and (under the default policy) unknown members are errors. A member
is optional when its field is `Option[T]`, or when it corresponds to an
`__init__` parameter with a default (see
[Invariants and visibility](#invariants-and-visibility)). Plenty has no field
defaults, so a class without `__init__` has no other optional members.

A `Nothing` field is written as `null` in JSON, so every declared member
appears in the document. TOML has no `null`, so its encoder omits the member
instead. Both formats decode an absent member as `Nothing`, so a document
written by one shape reads back in either.

`Bytes` and `bytes` are written as standard base64 text with padding. This
suits JSON and TOML, which have no byte string; a future binary format can add
a byte-string method to the model with a default that falls back to base64.

### Enums

Enums use the external tag shown above: the variant name, alone for a nullary
variant, otherwise as the only key of an object. This needs no reserved member
name, works in every self-describing format, and lets a decoder read the tag
before the payload, which an internally tagged form (`{"type": "Fixed",
...}`) cannot guarantee when members arrive in any order. Payloads are
positional, so they are written as a value or an array, never as named
members.

### Entry points

`std.serial` provides two compiler-expanded generic functions:

```text
def encode[T, E, S: Encoder[E]](value: &T, out: &mut S) -> Result[(), SerialError[E]]
def decode[T, E, D: Decoder[E]](source: &mut D) -> Result[T, SerialError[E]]
```

For a built-in type the compiler expands the mapping above; for a class or
enum it calls the type's hook. This is how a derived `__encode__` handles a
`list[Server]` field, and how a top-level array decodes without a wrapper
class. Like `copy` and `==`, they are defined for the types the rule admits
rather than through a protocol bound, because no protocol can admit `i64`.

Formats add typed entry points named `dump` and `load` beside their existing
ones. `options` carries the format's limits and the unknown-member policy:

```text
json.dump(&value, &options) -> Result[Bytes, SerialError[JsonError]]
json.load[Config](&source, &options) -> Result[Config, SerialError[JsonError]]
json.load_from[Config, R: Reader](&mut reader, &options)   # event-driven, bounded window
toml.dump(&value, &options) -> Result[str, SerialError[TomlError]]
toml.load[Config](&text, &options) -> Result[Config, SerialError[TomlError]]
```

`json.load_from` decodes straight from events, so memory is the result plus
the decoder's window. TOML parses to a tree first and writes from one, as its
module already requires. The reader variant's name follows `dump`/`load`; it
is settled when the entry points are implemented.

### Errors and field paths

```text
pub class SerialError[E]:
    pub cause: E
    pub path: list[Step]
    pub complete: bool

pub enum Step:
    Field(str)
    Index(i64)
    Key(str)
    Variant(str)
```

The cause is the format's own error, so JSON keeps its byte offset, line and
column; TOML, decoding from a tree with no positions, relies on the path. A
derived hook adds its step only while an error propagates out of it, so a
successful encode or decode never touches the path. Field and variant names
are string literals; a `Key` step moves the already decoded key. Adding a step
is a fallible list operation: if memory is refused, `complete` becomes `False`
and the cause is kept, so an out-of-memory condition never replaces the
original error. `SerialError` displays as
`servers[2].port: number out of range at line 7, column 12`.

### Invariants and visibility

- **Construction.** For a class without `__init__`, derived decoding uses the
  generated field constructor. An explicit `__init__` may enforce invariants
  that field-wise construction would bypass, so decoding never bypasses it.
  When the `__init__` parameters after `self` match the serialized fields
  exactly, by name and type, `@serial` derives `__decode__` by decoding each
  member and calling `__init__` with named arguments. A parameter with a
  default makes its member optional; when the member is absent, the call omits
  that argument and `__init__` applies the default. An error from `__init__`
  propagates with the class's path. Otherwise, `@serial` derives only
  `__encode__`, and the class must define `__decode__` by hand:

  ```text
  error: `@serial` cannot derive `__decode__` for `Server`
    note: `Server.__init__` has no parameter for field `tags`
    help: match the `__init__` parameters to the fields, or write `__decode__`
  ```

- **Members.** The serialized fields are all declared fields; names are the
  declared names, with no renaming.
- **Private fields.** Included. The author opted in, the derived code lives in
  the type's module, and structural printing and equality already include them.
- **Hook privacy.** Hooks follow the `__del__` rule: the compiler invokes them
  regardless of `pub`, so a module-private configuration class works with
  `json.load`. Without this, every serializable type would need to be
  public, since generic code in `std.json` reaches only public methods.
- **Destructors.** A failed decode drops the fields already decoded, as a
  failed initializer does; the destructor of the incomplete value never runs.

### Costs

| Operation | Allocates | Time |
|---|---|---|
| Derived `__encode__` | Nothing; the format grows its own output | Linear in the value |
| Derived `__decode__` | The value's own strings, lists and dictionaries; field slots are stack locals | Linear in the input; one `str` comparison chain per member name |
| Error propagation | One path step per enclosing hook, fallibly | Linear in the depth |
| JSON events | One `str` per key and string event, as today | As `json.Decoder.next` |
| TOML | The tree, as `parse` and `encode` already do | As today |
| Compilation | One encode and one decode body per type and format instance | Linear in fields; measure before adopting |

Member-name dispatch is a linear `match` over string literals. A type with
many fields could use a length-then-bytes switch; measure first.

## Rejected alternatives

- **Automatic serialization of every class**, as printing works. Field
  renames would silently change file formats, and private data would be
  exported without a decision.
- **A named `Serialize` protocol bound.** Cannot admit `i64`, `str` or
  `list[T]` without a special case, and cannot express receiver-less decoding.
- **Compile-time field iteration (A)** as the first step. Larger than
  serialization needs; revisit only with independent uses. If it is ever added,
  `@serial` can become library code without changing the hooks.
- **Per-format generated code (C).** No runtime gain over B after
  specialization; every new format would be a compiler change.
- **Runtime descriptor serialization (D).** Runtime reflection in all but name,
  no path to hand-written nested codecs, and format logic outside checked
  Plenty.
- **A richer data model now** (serde's records, tuple structs and newtypes as
  separate kinds). It serves positional binary formats; JSON and TOML do not
  need it. It can be added as new protocol methods with defaults once
  [protocol method bodies](protocol-method-bodies.md) ship.
- **Internally or adjacently tagged enums as the default.** Internal tags need
  a reserved member name and buffering when the tag arrives late.

## Decisions

Each records the choice and the alternatives considered.

1. **Opt-in spelling: `@serial`.** Rejected: separate `@encode` and `@decode`,
   and a `serial` clause in the class header. A split pair helps types that
   only ever write; `@serial` with a hand-written `__decode__` covers that.
2. **Renaming members: declared names only, for now.** Deferred: a closed
   field annotation such as `@serial.name("userId")`, which would be the first
   annotation on fields, and a per-call naming policy (snake to camel case).
3. **Unknown members: reject by default, with a per-call option to skip
   them.** This matches the strictness of both parsers. Rejected: skipping by
   default, as serde and Swift do, which hides typos in configuration files.
4. **`Nothing` fields: `null` in JSON.** Every declared member appears in a
   JSON document; TOML, which has no `null`, omits the member. Rejected:
   omitting in both formats for a uniform shape, which makes JSON output less
   explicit about the schema.
5. **Private fields: allowed and included.** The author opted in, and printing
   and equality already include them. Rejected: excluding them, and rejecting
   `@serial` on a class with private fields.
6. **Hook privacy: the `__del__` rule.** Generated and hand-written hooks are
   invoked regardless of `pub`, so private types work. Rejected: requiring
   `pub` types and hooks, as `std.fmt` display methods do.
7. **Classes with `__init__`: decode through `__init__` when its parameters
   exactly match the serialized fields; otherwise require a hand-written
   `__decode__`.** Parameters with defaults make their members optional, and
   an absent member takes the default. Rejected: deriving encode only for every
   class with `__init__`, which forces hand-written decoders for the common
   case of a validating constructor over the fields; and field-wise
   construction that bypasses `__init__`.
8. **Entry points: `dump` and `load`** in `std.json` and `std.toml`. Rejected:
   `to_bytes`/`from_bytes` and `to_text`/`from_text`, which differ between the
   two modules, and `encode_typed`/`decode_typed`, which collide with the
   `Encoder`/`Decoder` vocabulary.
9. **`Bytes`: base64 text.** Rejected for now: rejecting `Bytes` fields.
   Binary formats can add a byte-string model method later.
10. **`dict` with integer keys: rejected for now.** Deferred: writing keys as
    decimal text, as `serde_json` does.

## Implementation sketch

Each stage is usable on its own.

1. **`std.serial` in Plenty.** `Encoder`, `Decoder`, `Fault`, `Step`,
   `SerialError`, and the JSON decoder adapter. Verify that `json.Encoder`
   satisfies `Encoder[JsonError]`. Hand-written `__encode__`/`__decode__` can
   be tested as ordinary methods before any compiler work.
2. **Hooks and entry points.** Accept a receiver-less `__decode__` and an
   `__encode__` with the fixed signatures; expand `std.serial.encode` and
   `decode` per instance for built-ins and hooks; diagnostics naming the
   offending field path. Add `json.dump`, `load` and the reader variant.
3. **`@serial`.** Parse the annotation on classes and enums and generate hook
   bodies as ordinary checked operations, so ownership, effects and
   diagnostics need no special path; decoding through a matching `__init__`
   is an ordinary call with named arguments. `--dump` shows the generated
   bodies.
4. **TOML.** Tree builder and tree cursor; `toml.dump` and `load`.
5. **Docs.** Reference pages for `std.serial`, the `json` and `toml` additions,
   classes and enums; a tutorial lesson that replaces a hand-written codec.

### Measurements

Before stage 3 is accepted:

1. Compile time and object size for 200 classes of 10 fields, each encoded and
   decoded through JSON and TOML, against the same program with no derives
   (`compile_bench` method in [profiling](../development/profiling.md)).
2. Decode and encode throughput for the 8 MB document in the
   [`std.json` costs](../design/stdlib/json.md#costs), comparing derived code
   with `json.parse` to a tree and with a hand-written event codec. Derived
   code should match the hand-written codec.
3. Member-name dispatch cost for 4, 16 and 64 fields.
