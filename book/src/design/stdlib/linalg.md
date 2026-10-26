# `std.linalg` — dense linear algebra

`std.linalg` provides finite `f64` matrices, column vectors, multiplication,
linear solves and decompositions. Storage and the public API are written in
Plenty; the substantial numerical algorithms use faer 0.24.4 through an
[optional shared engine](linalg-engine.md). Constructing, inspecting and
transposing, adding, subtracting and scaling matrices works without installing
that engine.

## Matrices and vectors

`matrix(rows, cols, values)` copies a flat **row-major** list. Both dimensions
must be positive and the list must have exactly `rows * cols` entries.
`vector(values)` creates an `n × 1` matrix. `zeros(rows, cols)`, `identity(n)`
and `diagonal(values)` create common matrices. Inputs must be finite.

`Matrix` stores its values privately in column-major order. `rows()` and
`cols()` return dimensions; `get(row, col)` and mutable `set(row, col, value)`
use zero-based indices and reject negative or out-of-range indices. `to_list()`
returns a row-major copy. `transpose()` and `copy(matrix)` allocate independent
copies. Neither returns a borrowed view. Matrices have ordinary Plenty ownership
and can be passed, returned, or stored in other classes.

## Arithmetic

Arithmetic borrows its operands and returns a fresh matrix in
`Result[Matrix, AllocError | LinalgError]`. Use `(a + b)?`: parentheses put
error propagation around the complete operation. Vectors follow the same rules
because they are one-column matrices.

| Expression | Named equivalent | Meaning |
|---|---|---|
| `(a + b)?` | `a.add(&b)?` | Elementwise addition; identical shapes required |
| `(a - b)?` | `a.subtract(&b)?` | Elementwise subtraction; identical shapes required |
| `(-a)?` | `a.negate()?` | Negate every entry |
| `(a * b)?` | `a.matmul(&b)?` | Matrix product, not elementwise multiplication |
| `(2.0 * a)?` | `a.scale(2.0)?` | Multiply every entry by a finite `f64` scalar |

Scalar multiplication uses **scalar-left** syntax. `a * 2.0` is unsupported:
the current language permits one `__mul__` signature, which Matrix reserves
for matrix products. Use `a.scale(2.0)?` when scalar-right order reads better.
There is no broadcasting, elementwise multiplication, scalar addition, or
matrix division. Nonfinite scale factors return `NonFinite`; nonfinite computed
entries return `Overflow`.

No in-place arithmetic hooks are provided: these operations can fail, whereas
`+=` and similar hooks must return unit. Explicitly assign `a = (a + b)?` or
call a named method. Each operator allocates its own result; chains are not
fused and intermediate results consume storage. Dispatch is a direct borrowed
method call with no dispatch allocation or implicit operand copy, but the
result-producing wrappers are not promised to inline.

## Products, solves and factors

| Operation | Result and contract |
|---|---|
| `a.matmul(&b)` | Matrix product; `a.cols() == b.rows()` |
| `a.solve(&b)` | Solve square `A X = B` with partial-pivot LU; multiple RHS columns supported |
| `a.lu()` | Reusable `Lu`; rectangular matrices supported |
| `factor.solve(&b)` | Reuse a square LU factorization |
| `factor.lower()` / `upper()` | Explicit `m × k` unit-lower and `k × n` upper factors; `k = min(m,n)` |
| `factor.permutation()` | Row indices `p` such that `A[p, :] = L U` |
| `a.qr()` | `(q, r)`, full `m × m` orthogonal Q and `m × n` upper-trapezoidal R; `A = Q R` |
| `a.cholesky()` | Lower L with `A = L Lᵀ`; requires symmetric positive-definite A |
| `a.svd()` | `(u, values, v)`, thin factors with k columns; `A = U diag(values) Vᵀ` |
| `a.eigh()` | `(values, vectors)` for symmetric A; eigenvectors are columns and values ascend |
| `a.eig()` | `(real, imag, vectors_real, vectors_imag)` for any square real A; right eigenvectors are columns |

SVD singular values are nonnegative and descending. General eigenvalues have
no promised order. `eig()` represents complex results with separate real and
imaginary parts; it does not require a complex-number scalar type. Decomposition
signs/phases, eigenvalue order in `eig()`, and bases in repeated eigenspaces are
not stable identifiers. Compare residuals rather than exact factor entries.

Every operation preserves its inputs, including on failure. Results own their
storage. `solve` is preferable to forming an explicit inverse; factor once when
solving repeatedly. There are no implicit broadcasting,
indexing overloads, sparse matrices, empty matrices, or GPU operations.

## Errors and numerical limits

Allocating operations return `AllocError`, either directly or in a union with
`LinalgError`. Plenty matrix allocations and native scratch allocations are
fallible. Errors carry no dynamically allocated diagnostic message.

`LinalgError` distinguishes `Shape`, `Bounds`, `NonFinite`, `Singular`,
`NotSymmetric`, `NotPositiveDefinite`, `NoConvergence`, `Overflow`, and
`NativePanic`. Dimension/product bounds are checked before allocation.
Constructors/setters reject NaN and infinity; numerical results containing
NaN or infinity report `Overflow`. Symmetry is checked with exact `f64` equality
between the two triangles; symmetrize deliberately if your data is approximate.

LU reports `Singular` on an exactly zero diagonal pivot at solve time. Near
singularity is **not** reliably diagnosed: a finite result can be inaccurate.
No automatic regularization, rank tolerance, conditioning estimate, least-squares
solver or pseudoinverse is provided. General eigenvectors of a defective matrix
need not form a basis. Scaling and residual checks remain the caller's numerical
responsibility. Results may differ in their final bits across CPUs/releases.

See [the linear algebra lesson](../../tutorial/07-modules/27-linear-algebra.md)
for runnable solve and factorization examples, and the
[generated signatures](signatures.md) for every declaration.
