# Update a dictionary by transferring another dictionary

`destination.update(source)` returns `Result[(), AllocError]`, replacing
existing values and adding new entries. Existing keys keep their positions;
new keys appear in source insertion order:

```plenty
def merge(destination: &mut dict[str, i64], source: dict[str, i64]) -> Result[(), AllocError]:
    destination.update(source)

def main() -> Result[(), Failure]:
    mut scores = {"Ada": 10, "Bea": 20}?
    changes = {"Bea": 25, "Cam": 30, "Ada": 15}?
    merge(&mut scores, changes)?
    print(scores)?
    Ok(())
```
```output
{"Ada": 15, "Bea": 25, "Cam": 30}
```

The source is consumed on both outcomes. On allocation failure, the destination
stays unchanged and source owners are dropped. Use
`destination.update(copy(source)?)` to keep the source. Storage for every
new entry is reserved before any value is replaced. Old destination values are
dropped during successful replacement; their destructors keep their own effects.
Updating only existing keys needs no new runtime storage. Only same-typed
dictionaries are accepted; pair iterables and keyword arguments are deferred.
