# Identify records with UUIDs

Use `std.uuid` when records need identifiers that can be created independently,
without asking a central counter. Choose `v4()` for a random identifier, or
`v7()` when placing recently created records close together in a sorted index
is useful. Both use operating-system entropy and report failure through
`Result`.

```plenty
import std.uuid as uuid
from std.secrets import SecretsError

def main() -> Result[(), IoError | SecretsError | RangeError]:
    request_id = uuid.v4()?
    record_id = uuid.v7()?
    print(request_id.version(), record_id.version())?
    Ok(())
```
```output
Option[u8].Some(4) Option[u8].Some(7)
```

The identifiers change each run, so this example prints only their versions.
v7 embeds a wall-clock timestamp in milliseconds. It does **not** promise
creation order within a millisecond, or when the clock moves backwards. Use
`std.secrets` tokens rather than UUIDs for passwords and access credentials.

## Read, store and compare

Keep identifiers as `Uuid` values while working with them. Convert to canonical
text for display, or to 16 bytes for binary storage:

```plenty
import std.uuid as uuid
from std.bytes import BytesError

def main() -> Result[(), IoError | ParseError | BytesError]:
    id = uuid.parse("017F22E2-79B0-7CC3-98C4-DC0C0C07398F")?
    print(id.to_string()?)?
    raw = id.to_bytes()?
    restored = uuid.from_bytes(&raw)?
    print(raw.len(), restored == id)?
    print(id.unix_milliseconds())?
    Ok(())
```
```output
017f22e2-79b0-7cc3-98c4-dc0c0c07398f
16 True
Option[u64].Some(1645557742000)
```

Parsing accepts either case but requires the hyphens exactly as shown. It
rejects whitespace, braces and malformed digits. Formatting always produces
lowercase text. The byte representation is big-endian, matching the text's
left-to-right byte order. Use `to_string()` explicitly: ordinary class printing
is intended for inspecting fields, not interchange.

For a sorted list, borrow each UUID and use its pair of unsigned integer
halves as the key. This avoids allocating a string per comparison:

```plenty
import std.uuid as uuid
from std.bytes import BytesError

def uuid_key(id: &uuid.Uuid) -> (u64, u64):
    id.as_u64_pair()

def main() -> Result[(), AllocError | IoError | ParseError | BytesError]:
    mut records = [uuid.parse("017f22e2-79b1-7000-8000-000000000000")?,
                   uuid.parse("017f22e2-79b0-7cc3-98c4-dc0c0c07398f")?]?
    records.sort_by_key(uuid_key)
    for id in records:
        print(id.to_string()?)?
    Ok(())
```
```output
017f22e2-79b0-7cc3-98c4-dc0c0c07398f
017f22e2-79b1-7000-8000-000000000000
```

UUIDs follow ordinary class ownership rules. Their comparison, parsing and
numeric sorting keys need no allocation; creating output text or byte buffers
does. See the [reference](../../design/stdlib/uuid.md) for allocation-free
writing into existing buffers and deterministic generator testing.
