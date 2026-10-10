# Choose a public API

Mark each public field and method explicitly. Without `__init__`, the generated
field constructor is public only when the class and every field are public. To keep
fields private while allowing construction, declare `pub def __init__`:

Save this module as `counter.plenty`:

```plenty-file counter.plenty
pub class Counter:
    value: i64

    pub def __init__(self, start: i64) -> ():
        self.value = start

    pub def increment(self: &mut Counter) -> ():
        self.value = self.value + 1

    pub def read(self) -> i64:
        self.value

    def __del__(self) -> ():
        print("counter closed").unwrap()
```

Application:

```plenty
from counter import Counter

def main() -> Result[(), Failure]:
    mut count = Counter(41)
    count.increment()
    print(count.read())?
    Ok(())
```

```output
42
counter closed
```

The private destructor still runs automatically. Private fields and methods are
accessible throughout their module, but not from another module. Reads, writes,
and borrows all enforce this. Save the next module as `secret.plenty`:

```plenty-file secret.plenty
pub class Secret:
    value: i64

    pub def __init__(self, value: i64) -> ():
        self.value = value
```

```plenty-error
from secret import Secret

def main() -> ():
    item = Secret(42)
    print(item.value).unwrap()
```

```error
secret.Secret.value` is private
```

`pub` also applies to functions, enums, and type aliases. A public enum exposes
all its variants. Public signatures cannot mention private classes or enums,
even through aliases or containers. Ordinary imports are private bindings;
importing a name does not re-export it. Relative imports, wildcards, circular
imports, and `pub import` are not supported yet.

Private fields are still included in automatic printing and equality.
