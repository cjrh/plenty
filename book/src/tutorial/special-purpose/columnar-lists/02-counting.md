# Count matching integer values

Write the ordinary calculation you need. For a simple comparison over an `i32`
or `u32` column, `--release` can count matches with SIMD: the CPU compares four
values together. You do not choose a vector width or handle leftover rows.

```plenty
class Sample:
    score: i32

def main() -> Result[(), AppendError[Sample] | IoError]:
    mut samples = clist[Sample]()
    for score in range(9):
        samples.append(Sample(i32(score)))?
    matches = samples.column(Sample.score).filter(def(x: &i32) -> bool: *x > 5i32).count()
    print(matches)?
    Ok(())
```
```output
3
```

This is the same program in normal and optimized builds. The first SIMD slice
recognizes one comparison against a constant, with no captures or extra adapters.
Other calculations, including the floating-point average in
[the previous lesson](01-scanning.md), keep ordinary scalar execution.
All scans still run on one thread. See the
[columnar-list reference](../../../design/facilities/01-builtins/clists.md)
for the precise optimization limits, allocation, borrowing and representation.
