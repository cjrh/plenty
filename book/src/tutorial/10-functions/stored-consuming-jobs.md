# Store consuming jobs

A stored job may need to hand its captured resources to its caller. Use the
existing `def once` form and extract the job before calling it.

```plenty
def make_job(n: i64) -> Result[OnceClosure[[], list[i64]], AllocError]:
    values = [n]?
    job = def once [values]() -> list[i64]:
        values
    Ok(job)

def main() -> Result[(), Failure]:
    mut jobs = [make_job(10)?, make_job(20)?]?
    match jobs.pop(0):
        case Some(job):
            print(job())?
        case Nothing:
            pass
    for job in jobs:
        print(job())?
    Ok(())
```

```output
[10]
[20]
```

Calling `jobs[0]()` would try to consume a borrowed entry and is rejected.
Extract an owned callback with `pop`, consuming iteration, consuming enum matching,
or tuple unpacking.

Dropping uncalled jobs cleans up their captures. Borrowed captures and
generator-containing environments cannot be stored in these heap owners.
