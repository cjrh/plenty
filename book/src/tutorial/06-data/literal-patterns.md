# Match literal values

A pattern can be an integer, string, or boolean literal. It matches a value
equal to it, so `match` also selects on an integer, a `str`, or a `bool`
directly, and on such a value inside an enum payload:

```plenty
enum Command:
    Move(i64)
    Say(str)

def opcode(name: str) -> i64:
    match name:
        case "cpy":
            1
        case "inc" | "dec":
            2
        case _:
            0

def describe(command: Command) -> str:
    match command:
        case Command.Move(0):
            "stay"
        case Command.Move(_):
            "move"
        case Command.Say("" | "..."):
            "silence"
        case Command.Say(words):
            words

def main() -> Result[(), Failure]:
    print(opcode("cpy"))?
    print(opcode("dec"))?
    print(opcode("jmp"))?
    print(describe(Command.Move(0)))?
    print(describe(Command.Move(3)))?
    print(describe(Command.Say("...")))?
    print(describe(Command.Say("hello")))?
    Ok(())
```

```output
1
2
0
stay
move
silence
hello
```

Cases are tried in order. Literals cannot name every integer or string, so
such a match ends with `case _:`, and a payload position needs `_` or a name
after its literals:

```plenty-error
def main() -> ():
    count = 3
    match count:
        case 1:
            print("one").unwrap()
```

```error
non-exhaustive match; cases cannot name every i64 value, so end with `case _:`
```

`True` and `False` together cover a `bool`, so no `case _:` follows them:

```plenty
def label(enabled: bool) -> str:
    match enabled:
        case True:
            "on"
        case False:
            "off"

def main() -> Result[(), Failure]:
    print(label(True))?
    print(label(3 > 4))?
    Ok(())
```

```output
on
off
```

An integer literal takes the type of the value it matches and must fit it:
`case 300:` is an error for a `u8`. A float is not a pattern; compare floats
with `if`. A bare name still binds only at a payload position, so the catch-all for
the whole value is `case _:`, or [`case _ as name:`](bind-the-rest.md) to bind
it.

Each literal case costs one comparison, tried from the top, exactly like an
`if`/`elif` chain. Put the common cases first in a long match.
