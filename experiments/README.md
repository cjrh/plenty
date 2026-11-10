# Experiment inputs and arithmetic contracts

The `test_experiments` integration tests type-check the stored Plenty inputs and
representative generated columnar, module-cache and Rust-backend fixtures:

```sh
cargo test --locked --test integration test_experiments::
```

They supply the foreign-API stubs for `rust-crates/workload.plenty` and check the
intentional row-reference rejection. A directory with a `plenty.toml`, such as
`packages/memmap`, is checked as a package, tests included, with
`plenty check --tests`. `tests/fixtures/experiments/write_generated.py` lists the
generator calls; add a new generator's call there and its output names to
`GENERATED` in `tests/test_experiments.rs`.
They do not rebuild foreign libraries or reproduce performance measurements;
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

The [n-body and k-nucleotide investigation](nbody-codegen/README.md) separates
scalar loop overhead, SIMD capability, target ISA and runtime-helper costs.
Its stored kernels are ordinary checked programs; `run.py` generates a separate
unchecked diagnostic, and `unroll.py` generates fixed-size specialization probes.
The fully unrolled probes intentionally expose the current 256-local-slot limit
and are not correctness fixtures.
