# Experiment inputs and arithmetic contracts

Run the input and generator smoke check with a compiler built from the current
checkout:

```sh
cargo build --locked --bin plenty
python3 -B experiments/check_arithmetic.py target/debug/plenty
```

The check covers stored Plenty inputs and representative generated columnar,
module-cache and Rust-backend fixtures. It supplies the foreign-API stubs for
`rust-crates/workload.plenty` and checks the intentional row-reference rejection.
A directory with a `plenty.toml`, such as `packages/memmap`, is checked as a
package with `plenty check`, and each of its `tests/*.plenty` files as the
library of a temporary consumer package, so `import NAME` resolves as it does
under `plenty test`.
It does not rebuild foreign libraries or reproduce performance measurements;
use each experiment's runner for native execution and its specific invariants.

The [fannkuch kernels](kernels/README.md) propagate `OverflowError[i64]`, like
the current application example. The standalone column fold uses `try_fold`.
Polars input generators use positive typed divisors for their fixed moduli.

Experiments comparing infallible scalar ABIs retain explicit `unchecked:`
suites. Run these with the default `--unchecked trap` mode: overflow and division
by zero must still terminate, especially in the deliberate overflow probes.
Changing to `--unchecked assume` would invalidate those comparisons. The
arithmetic suites in the typestate model and generated negative-index lookup
instead document their representability invariants.

Recorded JSON, instruction counts and timing tables remain historical evidence;
this source migration does not refresh those measurements.
