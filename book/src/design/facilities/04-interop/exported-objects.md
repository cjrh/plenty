# Initialized C data exports

Some native protocols discover a named data object before calling any function.
A trusted interface module can export immutable bytes with native function and
data pointer relocations. The object exists as soon as the native loader has
loaded the library; its accessor need never run.

```text
pub opaque Pointer
pub extern def entry() -> Pointer = object "plugin_entry"(size=24, align=8):
    0: u32 = 1
    8: callback = "plugin_start"
    16: object = "plugin_name"
pub extern def name() -> Pointer = object "plugin_name"(size=5, align=1):
    0: bytes = b"demo\0"
```

`plugin_entry` is the object itself, not a pointer variable or a function.
Calling `entry()` returns its address. `plugin_start` must be an exported C
function in the same compilation; the relocation points to its C adapter.
An `object` relocation names another declared exported object, including a
forward reference or the object itself. These pointers must not be freed or
used after the originating library unloads.

## Layout and initialization

This initial facility supports 64-bit little-endian GNU/Linux targets. Native
execution tests currently cover x86-64. It does not derive layout from Plenty
classes or parse C headers. The binding author must match the native header's
size, alignment, field offsets and callback signatures for the chosen target.

- `size` is 1 through 1048576 bytes. `align` is a power of two from 1 through
  4096; size must be a multiple of alignment.
- Offsets, sizes, alignments and integer values are unsuffixed nonnegative
  decimal literals. This deliberately small initializer grammar has no
  expressions, casts, integer suffixes or runtime evaluation.
- Fields accept `u8`, `u16`, `u32`, `u64`, `bytes`, `callback` and `object`.
  Unsigned values must fit their width. Byte strings are copied exactly;
  no terminator is appended. Pointer fields occupy eight bytes.
- Fields must fit, must not overlap, and must have natural alignment, including
  adequate object alignment. Byte strings have alignment one. At most 4096
  fields are supported. Unspecified bytes, including padding, are zero.
- The object is immutable. Writing through its opaque accessor is invalid.
  The native linker places pointer relocations in read-only-after-relocation
  storage. Use [explicit writable storage](raw-host-interfaces.md#resident-addresses)
  for state initialized during host lifecycle calls.

Object symbols are exact names, without a required library prefix. They cannot
collide with functions, imported C symbols, another object, compiler-reserved
names or generated library metadata/handle symbols. Accessors take no parameters
and return an opaque pointer; these declarations are restricted to trusted
`.plentyi`/`.pi` modules.

## Packaging and safety

Both static archives and shared libraries retain objects whose accessors are
unused. For static archives, the native consumer still needs a reference that
causes its linker to extract the library object. A library may export only data.

Generated metadata fingerprints object size, alignment, initial contents and
relocations. The generated C header lists the object names and layout facts,
but does not invent C declarations: include the protocol's native header for
its authoritative types. Generated Plenty wrappers do not expose object
accessors; import the original trusted binding when building the implementation.

The compiler verifies relocation targets, not the native protocol's types or
lifetime rules. A callback field can only be used with its actual C signature,
under the export's threading contract, with all borrowed memory valid for the
call. No exception or nonlocal jump may cross Plenty frames. This facility does
not by itself establish real-time behavior or safe unloading.
