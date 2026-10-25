//! Direct Rust baseline: same Polars version, plan, dataset and collect loop.
use polars::prelude::*;
use std::time::Instant;

fn main() -> PolarsResult<()> {
    let n: usize = std::env::var("SPIKE_ROWS")
        .unwrap_or("1024".into())
        .parse()
        .unwrap();
    let reps: usize = std::env::var("SPIKE_REPS")
        .unwrap_or("100".into())
        .parse()
        .unwrap();
    let mode = std::env::var("SPIKE_MODE").unwrap_or("count".into());
    let start = Instant::now();
    let groups: Vec<i64> = (0..n).map(|i| (i % 8) as i64).collect();
    let values: Vec<f64> = (0..n).map(|i| (i % 100) as f64).collect();
    let input_ns = start.elapsed().as_nanos();
    let start = Instant::now();
    // Borrow slices to include copying into Polars-owned columns, like the bridge.
    let frame = df!("group" => groups.as_slice(), "value" => values.as_slice())?;
    let import_ns = start.elapsed().as_nanos();
    let query = if mode == "count" {
        frame
            .lazy()
            .filter(col("value").gt(lit(50.0)))
            .select([col("value").count().cast(DataType::Int64).alias("result")])
    } else {
        frame
            .lazy()
            .filter(col("value").gt(lit(50.0)))
            .with_columns([(col("value") * lit(2.0)).alias("weighted")])
            .group_by([col("group")])
            .agg([col("weighted").sum().alias("result")])
    };
    for _ in 0..5 {
        std::hint::black_box(query.clone().collect()?);
    }
    let mut checksum = 0.0;
    let start = Instant::now();
    for _ in 0..reps {
        let result = query.clone().collect()?;
        checksum += if mode == "count" {
            result.column("result")?.i64()?.get(0).unwrap() as f64
        } else {
            result
                .column("result")?
                .f64()?
                .into_no_null_iter()
                .sum::<f64>()
        };
    }
    let query_ns = start.elapsed().as_nanos();
    println!("{input_ns} {import_ns} {query_ns} {checksum}");
    Ok(())
}
