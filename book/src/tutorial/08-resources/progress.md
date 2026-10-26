# Show progress

Use `std.cli.progress` when a command does enough work that users need feedback.
Advance a bar **after** each successful unit of work and finish it explicitly.
The bar uses stderr only when it is a terminal, so redirected output remains
clean. The library limits redraws; an update on every item does not redraw on
every item.

```plenty
import std.cli.progress as progress
from std.cli.progress import ProgressError

def main() -> Result[(), ProgressError | IoError]:
    mut bar = progress.bar(5u64)
    mut total = 0
    for value in range(1, 6):
        total += value * value
        bar.advance()?
    bar.finish()?
    print("sum of squares:", total)?
    Ok(())
```
```output
sum of squares: 55
```

The calculation is deliberately short to keep the example runnable; a real
program might count imported records or processed files. For an unknown total,
use `progress.spinner()`: `advance()` counts completed units, and `tick()` requests
an animation step without changing the count. There is no background thread, so
a spinner only moves when your program updates it.

Propagate display errors as above unless your application deliberately treats
progress as optional. An early failure does not automatically finish the display;
finish before returning or writing diagnostics when a clean terminal line is
needed. Avoid mixing another progress bar or concurrent stderr logging with the
same display. The initial renderer assumes an ANSI terminal at least 80 columns
wide and uses compact approximate counts; `bar.position()` gives the exact count.

The [reference](../../design/stdlib/cli-progress.md) describes rates, ETA,
deterministic clock inputs and plain-text rendering.
