# Daily sales report

A complete `std.dataframe` application: read CSV or partitioned Parquet, parse
UTC timestamps, join a product catalogue, quarantine bad rows, and export
daily category totals. Query expressions run inside Polars; there are no
per-row Plenty callbacks. Python is only used by the checks and measurement
harness, not by the application.

## Run from the repository root

Build a current compiler and install the matching optional engine as described
in `book/src/design/stdlib/dataframe-engine.md`. For a source-built installation:

```sh
cargo build --release --locked --bin plenty
python3 -B scripts/build_dataframe_engine.py --out target/release
export PLENTY_DATAFRAME_ENGINE="$PWD/target/release/libplenty_dataframe_v1.so"
target/release/plenty --compile examples/sales_report/main.plenty -o /tmp/sales-report
target/release/plenty --compile examples/sales_report/convert.plenty -o /tmp/sales-convert
RUN=$(mktemp -d)
mkdir "$RUN/input" "$RUN/output"
/tmp/sales-convert ingest examples/sales_report/data/orders-01.csv "$RUN/input/part-01.parquet"
/tmp/sales-convert ingest examples/sales_report/data/orders-02.csv "$RUN/input/part-02.parquet"
/tmp/sales-report parquet "$RUN/input/part-*.parquet" examples/sales_report/data/products.csv "$RUN/output"
cat "$RUN/output/daily.csv"
cat "$RUN/output/rejected.csv"
```

The report prints 12 input lines, 5 accepted, 7 rejected and 4 daily groups.
The summary is:

```csv
day,category,lines,total_cents
2024-02-28,books,1,1200
2024-02-29,books,2,1000
2024-02-29,games,1,2500
2024-03-01,games,1,0
```

For a single CSV file, use `sales-report csv ORDERS.csv PRODUCTS.csv OUTPUT_DIR`.
Only Parquet accepts a glob; quote it so the shell passes one argument. Input
partitions must have compatible schemas. The catalogue is a single CSV file.
Compile first: the compiler's run command does not forward application arguments.

## Data contract

`report.plenty` contains the reusable query functions; `main.plenty` is the
small positional CLI. `convert.plenty` converts raw CSV to Parquet and verifies
the typed report during tests.

| Input | Required columns |
| --- | --- |
| Order lines, CSV or Parquet | String `order_id`, `occurred_at`, `product_id`, `amount_cents` |
| Catalogue, CSV | String `product_id`, `category` |

CSV scans disable inference: IDs such as `001` stay strings. Parquet here is a
**raw ingestion format**, with these same four String columns, not an arbitrary
typed sales table. Additional order columns are ignored. Schema errors fail
the run; bad cell values follow the quarantine policy below. Input files must
remain unchanged during scanning. Repeated order IDs represent separate lines;
this example does not deduplicate transactions or validate a financial ledger.

Timestamps represent UTC in `%Y-%m-%d %H:%M:%S` form, without offsets or leap
seconds. The example requests microsecond precision and uses calendar Date
values for grouping. It neither converts local time nor implements DST rules.

Catalogue IDs must be present, nonempty and unique; categories must be present
and nonempty. Categories are lowercased. No whitespace stripping is performed.
Validating uniqueness before the left join prevents accidental double counting.
The catalogue is expected to be small and is collected before orders are read.

Every rejected row retains the four original fields and one reason, chosen in
this order: `missing_order_id`, `invalid_timestamp`, `invalid_amount`,
`unknown_product`. Missing/invalid dates and amounts become null during parsing;
they are never filled with plausible business values. Unknown products remain
visible after the left join. CSV cannot preserve every lexical distinction
between null and quoted empty text; use the raw inputs as the original record.

Amounts are whole cents, from 0 through 1,000,000,000 inclusive. Negative amounts
(including refunds) are rejected. At most 100,000,000 input lines are accepted
by the application, bounding the sum below the i64 limit. This check happens
**after collection** and is not an input-memory limit. Native expression
arithmetic otherwise lacks Plenty's checked scalar overflow semantics.

## Outputs and failure handling

- `daily.parquet`: logical Date `day`, String `category`, i64 `lines` and
  `total_cents`, sorted by day/category.
- `daily.csv`: the same report, with explicitly formatted ISO dates.
- `rejected.csv`: original fields and rejection reason, sorted by order ID and
  timestamp, nulls last. Equal sort keys retain their upstream order, which can
  vary across partitions.

The output directory must exist. Writers refuse existing target files.
The three-file bundle is not transactional: a failed run can leave earlier
outputs or a partial new file. Use a fresh directory for each run and publish
the directory only after success. Rejected rows are an intentional successful
outcome, reported in stdout and the rejection file. Structural/query/I/O errors
propagate through the fallible entrypoint with their concrete error and a
nonzero status. Usage errors return 2; help and successful reports return 0.

The classified input is materialized once and reused for the rejection file
and daily query. `collect_streaming()` still retains the complete result;
sorts, joins and groups also allocate. Rejected rows are collected for CSV
export. This is a moderate-size batch application, not a bounded-memory
streaming pipeline. Native allocation failure can abort.

For deployment, bundle the executable with the matching
`libplenty_dataframe_v1.so` and its notices. See
`book/src/design/stdlib/dataframe-deployment.md`; end users need neither
Python nor Plenty/Rust compilers.

## Validate and measure

```sh
python3 -B examples/sales_report/check.py --plenty target/release/plenty -v
python3 -B examples/sales_report/check.py --plenty target/release/plenty --benchmark-rows 100000 -v
```

The harness compiles both programs with the Plenty function cache disabled,
uses temporary directories, compares outputs to Python stdlib calculations,
and checks Date metadata by reading Parquet with a typed getter. It covers
partitioned input, leap-day/midnight boundaries, bad dates/amounts, nulls,
Unicode and CSV quoting, duplicate catalogue keys, empty/all-rejected input,
schema/I/O failures, CLI status and overwrite protection. The optional engine
CI job runs the application checks. No Python Polars package is required.

The optional benchmark generates valid order lines, runs the whole application
three times with two Polars threads and verifies the final outputs. It reports
compile-and-link elapsed time, executable size, runtime and peak RSS using
`/usr/bin/time`. Generation and compilation are excluded from execution times;
file reads, query execution and all three output writes are included. These
are warm local filesystem measurements, not an out-of-core guarantee or a
comparison with other languages. Engine build time is excluded because its
compiled shared library is reused.

Local measurements on 2026-10-26, AMD Ryzen 7 7840HS, Linux x86-64, release
compiler built from `ae8792c9` (plus this example), matching datetime engine:

| Valid input lines | Three elapsed times | Median | Largest peak RSS |
| --- | --- | --- | --- |
| 100,000 | 0.05, 0.06, 0.05 s | 0.05 s | 73,680 KiB |
| 1,000,000 | 0.27, 0.26, 0.32 s | 0.27 s | 302,484 KiB |

Compiling and linking the report took 0.083–0.112 s with the function cache
off; the executable was 3,600,112 bytes, sharing the already built
148,384,160-byte engine. These are smoke measurements on synthetic valid data
with 56 output groups and no rejected rows. They do not measure a
rejection-heavy dataset, large catalogue, cold disk, or peak memory at the
application's row cap. Wall-clock variation and shared-host interference are
not controlled. The retained harness makes the measurements repeatable.
