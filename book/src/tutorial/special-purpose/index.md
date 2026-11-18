# Special-purpose data structures

Choose a data structure to suit the operations your program performs most often.
This section introduces collections with specialized storage and access patterns,
along with the tradeoffs that make them useful.

Start with [Columnar lists](columnar-lists/index.md): keep an ordinary class-shaped
interface while storing each primitive field in its own contiguous column.

For analytical queries and Parquet datasets, use
[Dataframes](dataframes.md): explicit expression plans executed by an optional
Polars engine. Columnar lists remain the lightweight mutable collection.

[Windowed counters and a command picker](gui-counters.md) contrasts a retained
model/view with immediate-mode clicks and editable text. The examples use an optional Linux GUI
source toolchain.
