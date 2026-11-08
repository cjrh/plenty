# Preserve partially prepared work

Splitting a [scheduler helper](scheduler-loops.md) into preparation, submission
and bookkeeping must not change its failure contract. If preparation succeeds
for some inputs and then fails, the caller still owns those prepared values:

```plenty
def prepare(inputs: &list[str], prepared: &mut list[i64]) -> Result[(), AllocError | ParseError]:
    for text in inputs:
        value = i64.parse(*text)?
        prepared.append(value)?
    Ok(())

def main() -> Result[(), AllocError | IoError | ParseError]:
    inputs = ["7", "invalid", "9"]?
    mut prepared: list[i64] = []?
    result = prepare(&inputs, &mut prepared)
    # Inspect/settle prepared work before propagating a later setup error.
    print(prepared)?
    match result:
        case Err(ParseError.Invalid):
            print("original parse error retained")?
        case _ as other:
            other?
    Ok(())
```
```output
[7]
original parse error retained
```

Integers keep this failure demonstration small; they require no external
cleanup. A real prepared run may own a child, a temporary directory, or a
reservation. Keep an explicit owner until it is either handed off or cleaned
up. Do not apply `?` to the batch result before settling that partial output.
If appending the newly prepared run fails, that run's obligations must also
be settled: use the collection's ownership-returning error where available,
reserve bookkeeping space before acquiring resources, or rely on a destructor
only when it really performs the required cleanup. Existing entries must stay
reachable. A list drop alone does not undo external side effects.

Likewise, successful submission transfers a job to the pool even if subsequent
bookkeeping fails. Record enough state before that point, or explicitly stop
and drain accepted work on failure. A rejected submission still owns an
unstarted job; `.drop_job()` is appropriate only when releasing its captures
is the intended cleanup. It preserves the submission error variant, not a
separate application obligation.

The demonstration handles one expected parse error to check the partial
output. Application code should normally finish cleanup and return the stored
error, keeping `AllocError`, setup errors and submission errors distinguishable.
In particular, turning a failed `copy(&pending.job)` into a generic “worker
could not start” outcome changes semantics. A refactor passing tests does not
by itself establish error compatibility.
