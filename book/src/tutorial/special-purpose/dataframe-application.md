# Application: a daily sales report

The repository's `examples/sales_report/` combines dataframe operations into
a command-line application. It reads order lines, joins a product catalogue,
separates bad records, and exports daily sales totals. The directory includes
small input files, reusable query functions, a CLI and independent output checks.

Start with the [engine installation](../../design/stdlib/dataframe-engine.md).
From a source checkout with the compiler and matching engine installed:

```sh
target/release/plenty --compile examples/sales_report/main.plenty -o /tmp/sales-report
target/release/plenty --compile examples/sales_report/convert.plenty -o /tmp/sales-convert
RUN=$(mktemp -d)
mkdir "$RUN/input" "$RUN/output"
/tmp/sales-convert ingest examples/sales_report/data/orders-01.csv "$RUN/input/part-01.parquet"
/tmp/sales-convert ingest examples/sales_report/data/orders-02.csv "$RUN/input/part-02.parquet"
/tmp/sales-report parquet "$RUN/input/part-*.parquet" examples/sales_report/data/products.csv "$RUN/output"
cat "$RUN/output/daily.csv"
```

The application prints 12 input lines, 5 accepted lines, 7 rejected lines and
4 daily groups. The resulting CSV is:

```csv
day,category,lines,total_cents
2024-02-28,books,1,1200
2024-02-29,books,2,1000
2024-02-29,games,1,2500
2024-03-01,games,1,0
```

It also writes `daily.parquet`, retaining logical Date values, and
`rejected.csv`, retaining the original fields with rejection reasons. For a
single CSV input, use `sales-report csv ORDERS.csv PRODUCTS.csv OUTPUT_DIR`.
Only Parquet supports the quoted glob. The converter keeps the raw String
columns; it does not pre-clean or calculate the report.

## Follow the query through the application

Read `report.plenty` in this order:

1. `scan_orders` selects four required String columns. CSV inference is
   disabled so product IDs such as `001` keep their leading zeros. Parquet
   input uses that same raw ingestion schema.
2. `catalogue` checks nonempty IDs/categories and unique product IDs before
   joining. Duplicate reference keys would otherwise multiply sales totals.
3. `classify` parses UTC timestamps and integer cents non-strictly, then
   performs a left join. A native conditional expression assigns each row an
   acceptance or rejection reason. Missing prices or dates are not silently
   replaced with useful-looking values.
4. `daily` filters accepted rows, groups by calendar date and lowercased
   category, counts lines and sums cents. Sorting gives stable report order.
5. `run` exports the report and rejected records, then prints reconciliation
   counts. It propagates structural/query/I/O failures through its fallible
   entrypoint, giving a nonzero exit status.

These functions build native expressions rather than invoking Plenty once
per row. Named intermediate plans make the cleaning policy readable without
copying the whole dataset at each step. The classified rows are collected
once so both outputs describe the same input; that is an explicit memory cost,
not a promise that streaming execution bounds memory.

Amounts are already line totals, expressed as whole cents. Zero is valid;
negative amounts, missing amounts and amounts above one billion cents are
rejected. Together with a 100-million-line cap before aggregation, this keeps
native i64 sums in range. The row cap is checked after collection and cannot
protect against running out of memory while reading. Repeated order IDs are
separate lines, not automatically deduplicated transactions. This is a batch
reporting example, not an accounting system.

## Check and ship it

Run the independent checks with the same engine installation:

```sh
python3 -B examples/sales_report/check.py --plenty target/release/plenty -v
```

The checks compile the actual application, compare CSV and Parquet results
with Python stdlib calculations, and exercise invalid rows, duplicate keys,
empty inputs and failures. Add `--benchmark-rows 100000` to measure a larger
generated workload, including all output writes. Python is a development
dependency of these checks, not a runtime dependency of the report.

Use a new output directory for each run. Files are not overwritten, but the
three writes are not one transaction: a failure may leave partial output.
Publish the directory only after the process succeeds. Rejected rows are an
intentional successful outcome; inspect the reconciliation counts and file.

The application README describes the exact input schema, rejection precedence,
resource costs and measurement procedure. Follow
[shipping dataframe applications](../../design/stdlib/dataframe-deployment.md)
to distribute the executable with its matching engine and license notices.
