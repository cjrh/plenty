# Compare sets

Set relationships borrow both sets and allocate nothing:

```plenty
def main() -> Result[(), Failure]:
    needed = {"read"}?
    available = {"read", "write"}?
    print(needed.issubset(available))?
    print(available.issuperset(needed))?
    print(needed.isdisjoint({"write"}?))?
    print(len(needed))?
    Ok(())
```

```output
True
True
True
1
```

Both operands must have the same set type. Empty sets are subsets of every set
and disjoint from every set; neither operand is consumed.
