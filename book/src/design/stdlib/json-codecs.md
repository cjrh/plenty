# Typed JSON codecs

[`std.serial`](serial.md) supplies built-in codecs and private handwritten
class/enum hooks. `std.json` adapts those codecs to JSON events and to an
owning `Json` cursor, without serializing and reparsing a tree.

## Complete documents

```text
codec_options() -> CodecOptions
load[T](source: &[u8], options: &CodecOptions) -> Result[T, SerialError[JsonError]]
dump[T](value: &T, bounds: &Limits) -> Result[Bytes, SerialError[JsonError]]
```

`CodecOptions` has public `limits: Limits` and
`unknown_fields: std.serial.UnknownFields` fields. Defaults are `limits()`
and `Reject`. `Ignore` skips unknown values through the bounded event decoder.
A custom hook must call `unknown_key` for this policy to apply.

`load` feeds one complete buffer to the existing event decoder, decodes the
requested type, and checks that the document is complete. It creates no
intermediate `Json` tree. `dump` uses the existing validating encoder and rejects
hooks that leave an empty or unfinished document. Both
retain existing [JSON limits and faults](json.md#limits). Duplicate-key policy
in `Limits` applies to tree parsing; typed dictionaries and field slots reject
duplicates themselves even when it is `Keep`.

For adapter composition, `codec_decoder(source, options)` returns a
`CodecDecoder` satisfying `std.serial.Decoder[JsonError]`. Its `finish()`
checks that the codec consumed exactly one complete document. The existing
`Encoder` satisfies `std.serial.Encoder[JsonError]`; check `is_complete()`
after the codec succeeds and before calling `take()`. `take()` also supports
incremental output and does not check document completeness itself. Incremental
reader-based typed loading is not implemented.

## Direct value conversion

```text
tree_limits() -> TreeLimits
to_value[T](value: &T, limits: &TreeLimits) -> Result[Json, SerialError[JsonError]]
from_value[T](value: Json, limits: &TreeLimits,
              unknown_fields: std.serial.UnknownFields = Reject) -> Result[T, SerialError[JsonError]]
```

`from_value` consumes its input tree, including on failure. The cursor moves
children from the tree into the decoded value and never retains a borrowed
reference in a class. It reverses container storage once for forward traversal;
there is no textual representation or numeric formatting/parsing in either
direction. `to_value` borrows the input and creates an independent tree.

`TreeLimits` has public nonnegative fields `max_depth`, `max_nodes`, and
`max_text_bytes`, defaulting to 64, 1048576, and 67108864. Arrays and objects
count toward depth. Every value and object key counts as a node. Text values
and keys count their aggregate UTF-8 bytes. Node/text exhaustion returns
`OutputTooLarge` in either direction; excess depth returns `DepthExceeded`.
Tree error byte offsets, lines and columns are zero because no text is read.
A `max_depth` above 1024 traps at adapter construction.

`Json.Int` holds only `i64`: converting a larger `u64` to a tree returns
`Serialization(Fault.Unrepresentable)`, although textual JSON can represent
it. Non-finite floats return `NonFiniteNumber`. Decoding integers rejects
out-of-range values; numeric strings are not coerced.

`tree_encoder(limits)` returns `TreeEncoder`, which satisfies the encoder
protocol and returns its single value through `finish()`.
`tree_decoder(value, limits, unknown_fields = Reject)` returns `TreeDecoder`,
which satisfies the decoder protocol; `finish()` checks complete consumption.
Use each adapter for one operation and discard it after a failure.

## Typed errors

All entry points return `std.serial.SerialError[JsonError]`. Parsing/output
faults keep the existing typed `JsonError`; schema failures are
`JsonFault.Serialization(std.serial.Fault)`. Collection and handwritten field
paths identify nested failures without allocating path entries on success.
Handwritten hooks control field defaults, constructor validation and enum
representation. Derivation and automatic field metadata are not implemented.
