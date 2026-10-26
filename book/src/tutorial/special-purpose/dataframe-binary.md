# Store binary payloads

Use binary columns for file contents, encoded messages, or other bytes that
are not necessarily text. Values use owned [`Bytes`](../../design/stdlib/bytes.md)
buffers; there is no UTF-8 conversion at the dataframe boundary.
This lesson requires the [optional engine](../../design/stdlib/dataframe-engine.md).

Keep selection inside Polars, then extract only the payloads your application
needs. Here, null means missing data and an empty buffer is a present payload:

```plenty dataframe
import std.dataframe as df
import std.bytes as buffers
from std.bytes import Bytes, BytesError

def main() -> Result[(), df.DataFrameError | BytesError | AllocError | IoError]:
    payloads: list[Option[Bytes]] = [
        Some(buffers.from_bytes(b"\xff\0")?),
        Nothing,
        Some(buffers.empty()?)
    ]?
    data = df.frame([df.column_binary_nullable("payload", payloads)?]?)?
    needle = buffers.from_bytes(b"\xff\0")?
    matches = data.lazy()?.filter(
        df.col("payload")?.eq(df.lit_binary(needle)?)?
    )?.collect()?
    print(matches.height()?)?
    for payload in matches.values_binary("payload")?:
        print(payload.len(), payload[0])?
    print(data.get_binary("payload", 1)?.is_none())?
    print(data.get_binary("payload", 2)?.unwrap().len())?
    Ok(())
```
```output
1
2 255
True
0
```

Use `values_binary_nullable` when the selected result may contain nulls.
`values_binary` rejects nulls; it never silently drops rows. Both return new
buffers that can outlive the dataframe and be changed independently.

`write_parquet` and `scan_parquet` preserve binary values and nulls. CSV does
not support binary columns. Converting a binary expression to `DType.String`
is explicit and checks UTF-8; it is unnecessary for writing Parquet.

The current transfer path uses temporary byte lists and copies the data.
Prefer native filtering/projection before extraction, especially for large
payloads. See [binary transfer details](../../design/stdlib/dataframe-types.md#binary-payloads).
