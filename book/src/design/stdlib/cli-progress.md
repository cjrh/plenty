# `std.cli.progress`

Bars and spinners for long-running command-line work, implemented in Plenty over
[`std.io`](io.md), [`std.time`](time.md) and [`std.report`](report.md).
The only new native operation asks whether a process stream is a terminal.

```plenty
import std.cli.progress as progress
from std.cli.progress import ProgressError

def main() -> Result[(), ProgressError]:
    mut bar = progress.bar(100u64)
    for item in range(100):
        # Process one item here, then count it.
        bar.advance()?
    bar.finish()?
    Ok(())
```

## Construction and updates

`bar(total: u64)` and `spinner()` return an owned `Progress`, initially at zero.
A bar has a known total; a spinner counts work without predicting a total.
Construction does not draw. Neither creates a worker or timer thread.

| Method | Contract |
|---|---|
| `advance(count: u64 = 1u64)` | Add completed work; checked for overflow and the bar's total |
| `set_position(position: u64)` | Set an absolute count; it must not decrease or exceed the total |
| `tick()` | Request a redraw without changing the count; useful for a spinner |
| `finish()` | Draw the final count and a newline, ignoring the throttle; repeated calls do nothing |
| `position()` / `total()` | Exact `u64` count / `Option[u64]` total |
| `is_finished()` | Whether finished or stopped by a display error |

Updates and finish return `Result[(), ProgressError]`. Finishing early preserves
the actual count: it does not claim remaining work was done. A zero-total bar is
already full, but still needs `finish()` if it was drawn. Updates after finishing
return `Finished`; invalid positions/overflow return `InvalidPosition` and leave
the count unchanged. Output failure preserves the accepted count, stops further
updates, and returns `Output(std.report.FormatError)` with its original cause.

## Display and metrics

Output goes only to stderr, and only if stderr is a terminal both at construction
and before each draw. Redirected runs emit nothing, including on finish; stdout
stays available for application results. Closed/undetectable streams count as
nonterminals. The first update draws immediately. Subsequent updates draw at most
every 100 ms, using a monotonic clock; skipped updates still count completed work.
The spinner advances its frame on actual redraws, not every update call. If a
later query finds redirected stderr, that progress display stays hidden.

The default line uses a ten-cell ASCII bar or a four-frame spinner, counts,
average whole items/second and estimated whole seconds remaining. Counts and
metrics use truncated decimal suffixes `k`, `M`, `G`, `T`, `P`, `E` to keep lines
short, including at `u64.MAX`. The exact counts remain available through getters.
The line is under 80 columns. Interactive rendering assumes an ANSI-compatible
terminal at least 80 columns wide: carriage return and erase-line replace the
previous frame. Cursor visibility and terminal modes are never changed.

`rate(now: &Instant) -> Option[f64]` is the average count per second since
construction. `eta(now: &Instant) -> Option[f64]` is remaining seconds at that
average. Zero elapsed time yields no rate; an unknown total or zero rate yields
no ETA, displayed as `--`. Completed known totals have ETA zero. Estimates use
floating-point arithmetic; displayed values truncate and saturate to `u64`.
Queries use the supplied time even after finishing; they do not freeze metrics.

For tests or an application-owned clock, `bar_at(total, now)`, `spinner_at(now)`,
`set_position_at(position, now)` and `finish_at(now)` take a borrowed `Instant`.
Use one timeline consistently. Backwards elapsed time saturates at zero;
backwards redraw timestamps cannot bypass the throttle.
`render(now, output: &mut Formatter)` writes the plain line without newline or
cursor controls and leaves redraw state unchanged. A string formatter allows
snapshot testing; formatter errors propagate unchanged.

## Costs and limits

Progress state is fixed-size. Normal drawing uses the bounded reporting formatter,
without constructing temporary strings. Hidden and throttled updates skip
formatting. Stderr writes may fail or block; a frame consists of multiple small
writes and is not atomic with concurrent output. Keep one progress owner on one
thread, and finish it before writing other diagnostics to stderr. Dropping it
does not write: explicitly finish before normal early returns if a clean line is
needed. An output failure may leave a partial line.

This initial API has no automatic ticking, multiple-bar coordination, templates,
labels, colors, terminal resizing or iterator adapters. It follows the manually
updated, terminal-only stderr model of
[indicatif](https://docs.rs/indicatif/0.18.6/indicatif/), without embedding it.
See [Show progress](../../tutorial/08-resources/progress.md) for a small example.
