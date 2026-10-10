# Empty a collection and reuse its storage

Lists, dictionaries, and sets have `clear() -> ()`. The method requires mutable
access, drops the contents, and retains capacity for later insertions:

```plenty
def main() -> Result[(), Failure]:
    mut items = [10, 20]?
    items.clear()
    print(items)?
    items.append(30)?
    print(items)?
    mut scores = {"Ada": 10}?
    scores.clear()
    print(scores)?
    Ok(())
```

```output
[]
[30]
{}
```

Clearing itself allocates no storage. Owned elements are destroyed before the
method returns, in list order or dictionary insertion order; custom destructors
may have their own effects and allocations. Clearing an empty collection is fine.
