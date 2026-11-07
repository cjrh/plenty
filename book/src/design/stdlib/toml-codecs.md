# Typed TOML codecs

`std.toml.dump` and `std.toml.load[T]` use the checked
[`std.serial`](serial.md) hooks to convert application values to and from TOML.
The TOML parser and writer remain responsible for document syntax and byte
limits. No JSON intermediate representation is involved.

```plenty
import std.toml
from std.serial import SerialError
from std.toml import TomlError

def main() -> Result[(), AllocError | IoError | SerialError[TomlError]]:
    options = std.toml.codec_options()
    ports = {"http": 80u16, "https": 443u16}?
    text = std.toml.dump(&ports, &options)?
    restored = std.toml.load[dict[str, u16]](&text, &options)?
    print(restored == ports)?
    Ok(())
```
```output
True
```

## Representation

Strings, booleans, signed integers, floats, arrays, tuples, and string-keyed
objects use the corresponding TOML values. Unsigned integers must fit `i64`;
otherwise encoding fails with `Serialization(Unrepresentable)`. TOML supports
infinities and NaNs, so these floats round-trip. `dump` requires a table at the
document root and reports `NotATable` for a scalar or array root.

TOML has no null. A null emitted after `Encoder.field` is omitted. A null map value, root, or
array element fails with `Serialization(Unrepresentable)`. An absent optional
field is handled by the type's decoding hook; the adapter does not invent
missing fields. With derivation, a constructor default of `Some(...)` therefore
replaces an omitted `Nothing` on reload: that value cannot round-trip through
TOML. Use `Nothing` as the optional default when its absence must round-trip.
Date-time values remain available through the
[TOML value tree](toml.md#value-tree), but have no implicit scalar conversion
in the format-neutral protocol.

Handwritten class hooks use `Encoder.field(name)` for fields; maps use
`Encoder.key(name)`. This distinction prevents absent dictionary values from
silently deleting dictionary entries.

Field order follows the encoding hook or collection iteration order; decoded
tables retain their source order. Duplicate keys in text are rejected by the
parser; the tree encoder rejects repeated member names even when an earlier
optional field was omitted. `CodecOptions.unknown_fields` defaults to `UnknownFields.Reject`.
`Ignore` skips the selected value completely, including nested arrays, tables,
and date-times, while still enforcing tree traversal limits.

## Trees and bounds

`to_value[T](&T, &TreeLimits)` constructs an owned `Toml` value.
`from_value[T](Toml, &TreeLimits, unknown_fields)` consumes a tree and requires
its decoding hook to consume the complete selected value. The optional
`unknown_fields` argument defaults to `Reject`.

`tree_encoder` and `tree_decoder` expose the same adapters for handwritten
hooks. Call `finish()` after successful encoding or decoding; an unclosed
container, pending member value, absent root, or unread value reports
`InvalidState`. Discard an adapter after any error. The decoder owns its input
because Plenty classes cannot retain borrowed references.

`CodecOptions` combines the existing TOML `limits`, `tree_limits`, and unknown
field policy. `tree_limits()` defaults to depth 64, 1,048,576 nodes, and 64 MiB
of text. Nodes count containers, scalars, and object keys; text counts UTF-8
bytes in strings and keys. Omitted null members still consume their key and
value budgets. `max_depth` is capped at 1024, like the parser and writer.
Exceeding tree depth reports `DepthExceeded`; node or text budgets report
`OutputTooLarge`, including during decoding. These limits bound traversal and
new tree construction; consuming an already-owned tree does not undo its prior
allocation.

`load` parses the complete bounded input into a TOML tree before decoding.
`dump` constructs a bounded tree before writing the document, and applies
`limits.max_output` to the resulting UTF-8 bytes. Input and output byte limits
remain independent of the tree limits.

## Errors

Typed operations return `SerialError[TomlError]`. Syntax errors retain their
byte offset, line, and column. Tree/schema errors have zero source coordinates
and use the serial error's typed `Step` path. `TomlFault.Serialization(Fault)`
carries schema failures, and `TomlFault.Allocation(AllocError)` preserves
allocation failures from tree storage, decoding collections, and output.
Failure to allocate a contextual path preserves the original cause and sets
`SerialError.complete` to `False`.

Use [serialization diagnostics](serial-diagnostics.md) to render the complete
typed cause and root-first path with explicit byte limits. Kind mismatches
retain expected and actual kinds; source snippets are opt-in and only appear
when input coordinates are known.
