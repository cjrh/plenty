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

The job body transfers its list without copying or allocating. Queue construction
allocates, but extraction and invocation do not. Calling `jobs[0]()` directly
would try to consume a borrowed entry and is rejected. `pop`, consuming iteration,
consuming enum matching, and tuple unpacking all produce owned callbacks.

Calling an extracted job twice is rejected. Dropping a queue with uncalled jobs
cleans up their captured resources. Borrowed captures and generator-containing
environments cannot be stored in these heap owners.
