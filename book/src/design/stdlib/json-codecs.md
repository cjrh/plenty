# Typed JSON codecs

[`std.serial`](serial.md) supplies built-in codecs, [`@serial`](serial-derive.md)
derivation, and private handwritten class/enum hooks. `std.json` adapts those codecs to JSON events and to an
owning `Json` cursor, without serializing and reparsing a tree.

## Complete documents

```text
codec_options() -> CodecOptions
load[T](source: &[u8], options: &CodecOptions) -> Result[T, SerialError[JsonError]]
dump[T](value: &T, bounds: &Limits, indent: Option[NonNegative[i64]] = Nothing) -> Result[Bytes, SerialError[JsonError]]
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
after the codec succeeds and before calling `take()`. Pass `indent=Some(2)`
to `dump` or `encoder` for [bounded pretty output](json.md#pretty-output). `take()` also supports
incremental output and does not check document completeness itself.

## Incremental reader loading

```text
codec_reader[R: Reader](inner: R) -> CodecReader[R]
load_from[T, R: Reader](source: &mut CodecReader[R], options: &CodecOptions)
    -> Result[T, SerialError[JsonError]]
```

Create an owning adapter with `codec_reader(reader)`, then pass a mutable borrow
of it to `load_from`. Its public `inner: R` retains the stream owner on success
and failure. The infallible factory does not allocate or read; parser allocation
happens inside `load_from` while the adapter is borrowed. This ownership follows
[`std.io` stream adapters](io.md): Plenty classes cannot store a borrowed reader.

`load_from` decodes directly from events, retaining the requested result, a
4096-byte read window, the current token, and the bounded parser stack. It
creates neither a complete input buffer nor an intermediate `Json` tree.
`max_input` bounds total bytes read, including whitespace; `max_depth` bounds
nesting. A token can require storage up to the input limit. Unknown values under
`Ignore` are skipped through the same bounded parser. Typed collection storage
belongs to the returned value and remains subject to fallible allocation.

The loader accepts short reads, retries `StreamError.Interrupted`, and checks
reader progress. Other stream failures retain `JsonFault.Stream(cause)` with
input location and the available typed field/index path. It reads through EOF
to validate exactly one complete document; trailing data, malformed skipped
values and a read failure during completion still fail the load. An exact
`max_input`-byte document succeeds only after the stream confirms EOF; one extra
byte reports `InputTooLarge`.

Each adapter accepts one `load_from` call after initialization. A second call
returns `InvalidState` without reading. Discard the adapter after a parse,
schema or stream error. Reads can prefetch bytes past the reported error, so
`inner` exposes the physical stream position, not a restart point. Retrying an
interrupted individual read is automatic; retrying the whole failed document
requires a new adapter and a stream explicitly reset by its owner. Initialization
allocation failure leaves the adapter uninitialized and the stream untouched.

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

Typed loading, dumping and value conversion return
`std.serial.SerialError[JsonError]`. Parsing/output
faults keep the existing typed `JsonError`; schema failures are
`JsonFault.Serialization(std.serial.Fault)`. Collection and field
paths identify nested failures without allocating path entries on success.
Derived hooks preserve constructor validation and absent-field defaults, with
the representations described in [`@serial`](serial-derive.md). Handwritten
hooks can override either codec direction.

Use [serialization diagnostics](serial-diagnostics.md) to render the complete
typed cause and root-first path with explicit byte limits. Kind mismatches
retain expected and actual kinds; source snippets are opt-in and only appear
when input coordinates are known.
