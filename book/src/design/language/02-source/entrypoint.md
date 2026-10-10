# Application entrypoint

A binary application requires exactly one module-level `main` in its entry
module, with no parameters and one of these return types:

| Return type | Process result |
| --- | --- |
| `()` | Status zero |
| `i32` | The returned status |
| `Result[(), E]` | `Ok(())` gives zero; `Err` gives one |
| `Result[i32, E]` | `Ok(status)` gives that status; `Err` gives one |

Transparent aliases are accepted. The wrapper calls `main` once. Small
nonnegative statuses are portable; an operating system may truncate others.

A returned `Err` is reported once on standard error after flushing standard
output. Its payload is destroyed exactly once, and reporting failure does not
change status one. Intermediate error propagation prints nothing. See
[entrypoint reporting](../../toolchain/01-building/entry-report.md) for the exact
format and fallback behavior.

Checking and compilation diagnose missing or invalid entrypoints before running
code or producing an output artifact. Both diagnostics list all four accepted
return types. Compilation errors execute nothing; runtime errors can leave
effects that already occurred. Library checking does not require `main`.
