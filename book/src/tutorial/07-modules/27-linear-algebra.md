# Solve systems and factor matrices

`std.linalg` gives you dense matrices and column vectors of `f64` values. It
uses faer's numerical algorithms through an optional native engine. Install
the [linear algebra engine](../../design/stdlib/linalg-engine.md) before running
these examples. Matrix construction and element access work without it.

## Solve a linear system

Pass matrix entries in row order. Use `solve` to find `x` in `A x = b`:

```plenty linalg
import std.linalg as la

def main() -> Result[(), AllocError | IoError | la.LinalgError]:
    a = la.matrix(2, 2, &[3.0, 1.0, 1.0, 2.0]?)?
    b = la.vector(&[9.0, 8.0]?)?
    x = a.solve(&b)?
    print(x.get(0, 0)?, x.get(1, 0)?)?
    Ok(())
```
```output
2.0 3.0
```

Both inputs are preserved. The result is a two-row, one-column matrix. For
several right-hand sides with the same `a`, compute `factor = a.lu()?` once,
then call `factor.solve(&b)?` for each. This reuses the expensive factorization.

## Inspect a low-rank approximation

SVD separates a matrix into two orthogonal factors and a list of singular
values. Small singular values indicate directions that contribute little:

```plenty linalg
import std.linalg as la

def main() -> Result[(), AllocError | IoError | la.LinalgError]:
    a = la.diagonal(&[4.0, 2.0, 0.01]?)?
    (u, values, v) = a.svd()?
    print(values)?
    print(u.rows(), u.cols(), v.rows(), v.cols())?
    Ok(())
```
```output
[4.0, 2.0, 0.01]
3 3 3 3
```

The reconstruction is `u.matmul(&la.diagonal(&values)?)?.matmul(&v.transpose()?)?`.
For rectangular inputs the factors have only `min(rows, cols)` columns. `qr`,
`cholesky`, `eigh`, and `eig` provide other decompositions; their contracts are
in the [linear algebra reference](../../design/stdlib/linalg.md).

All matrix-building operations are fallible. Check shapes and handle errors
normally with `?`. Floating-point calculations are approximate: compare residuals
with a tolerance appropriate to your data, not exact factor entries. A successful
solve does not guarantee accuracy for a nearly singular matrix.
