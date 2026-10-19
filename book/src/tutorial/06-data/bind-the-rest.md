# Bind the value a match leaves over

`case _:` covers the remaining values but names nothing. To handle a few
variants and pass every other value on, end with `case _ as name:`, which binds
the whole matched value:

```plenty
enum Event:
    Tick
    Key(str)
    Resize(i64, i64)

def describe(event: Event) -> str:
    match event:
        case Event.Tick:
            "tick"
        case Event.Key(name):
            name
        case Event.Resize(_, _):
            "resize"

def without_ticks(event: Event) -> str:
    match event:
        case Event.Tick:
            "ignored"
        case _ as other:
            describe(other)

def main() -> Result[(), IoError]:
    print(without_ticks(Event.Tick))?
    print(without_ticks(Event.Key("enter")))?
    print(without_ticks(Event.Resize(80, 24)))?
    Ok(())
```

```output
ignored
enter
resize
```

`without_ticks` keeps working when `Event` gains a variant; only `describe`
must name it.

The binding follows the matched value. Matching an owned value moves it into
the name; matching `&value` or `&mut value` binds a shared or exclusive
reference to it:

```plenty
def bump(count: &mut Option[i64]) -> ():
    match count:
        case Some(number):
            *number = *number + 1
        case _ as empty:
            *empty = Some(1)

def main() -> Result[(), IoError]:
    mut count: Option[i64] = Nothing
    bump(&mut count)
    bump(&mut count)
    print(count)?
    Ok(())
```

```output
Option[i64].Some(2)
```

`_ as name` matches every value, so it is the last case and stands alone: a
case after it is unreachable, and it cannot be a `|` alternative. The `_ as` is
required. A bare name is rejected, because it is usually a variant written
without its enum:

```plenty-error
enum Switch:
    On
    Off

def main() -> ():
    match Switch.On:
        case On:
            pass
        case _:
            pass
```

```error
a case cannot bind the whole value to the bare name `On`; bind it with `_ as On`, match any value with `_`, or qualify a variant with its enum type
```

Inside a payload a name alone binds, as in `Event.Key(name)`; `_ as name` is
only for the whole value.
