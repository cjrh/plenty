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

## Use runtime bounds

Bounds do not need to be constants. Capture their scalar values to count a range:

```plenty
class Sample:
    score: i32

def count_between(samples: &clist[Sample], low: i32, high: i32) -> i64:
    samples.column(Sample.score).filter(def [low, high](x: &i32) -> bool: *x >= low and *x < high).count()

def main() -> Result[(), AppendError[Sample] | IoError]:
    mut samples = clist[Sample]()
    for score in range(9):
        samples.append(Sample(i32(score)))?
    print(count_between(&samples, 2i32, 7i32))?
    Ok(())
```
```output
5
```

`[low, high]` captures copies of the bounds. They can come from function
arguments or parsed input. The closure keeps those values even if an original
binding later changes. Write two comparisons with `and`; Plenty does not use
chained comparison syntax.

This is the same program in normal and optimized builds. The SIMD path recognizes
one comparison, or two comparisons joined by `and`, against constants or up to
two immutable scalar captures of the column's type. Borrowed captures, arithmetic
inside the predicate, `or`, and extra adapters keep scalar execution.
Other calculations, including the floating-point average in
[the previous lesson](01-scanning.md), keep ordinary scalar execution.
All scans still run on one thread. See the
[columnar-list reference](../../../design/facilities/01-builtins/clists.md)
for the precise optimization limits, allocation, borrowing and representation.
