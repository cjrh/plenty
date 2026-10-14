# Read the environment and run other programs

`std.env` reads the variables a program was started with. `std.process` starts
another program from a list of arguments and collects what it prints:

```plenty
import std.env
import std.process
from std.process import Stdio

def main() -> Result[(), Failure]:
    print(std.env.get("PLENTY_LESSON_UNSET")?)?
    mut command = std.process.command("printf")?
    command.arg("%s\n")?
    command.arg("two words; echo $HOME | wc")?
    command.stdout(Stdio.Piped)
    mut child = command.spawn()?
    output = child.output(4096)?
    print(output.stdout.to_text()?)?
    print(output.status.success(), output.status.code())?
    Ok(())
```
```output
Option[str].Nothing
two words; echo $HOME | wc

True Option[i32].Some(0)
```

An unset variable is `Nothing`, not an error. Each `arg` is one argument,
delivered byte for byte: the semicolon, the `$HOME`, and the `|` are printed,
not run, because no shell reads them. `Stdio.Piped` connects the child's output
to a pipe, and `output` collects at most 4096 bytes of it and waits for the
child to end.

When shell syntax is what you want, ask for it by name. `shell` runs
`/bin/sh -c` with your script. `communicate` also feeds the child's input:

```plenty
import std.bytes
import std.process
from std.process import Stdio

def main() -> Result[(), Failure]:
    mut command = std.process.shell("tr a-z A-Z; echo \"mode: $MODE\" >&2; exit 3")?
    command.env("MODE", "loud")?
    command.stdin(Stdio.Piped)
    command.stdout(Stdio.Piped)
    command.stderr(Stdio.Piped)
    mut child = command.spawn()?
    input = std.bytes.from_text("quiet words")?
    output = child.communicate(&input, 4096)?
    print(output.stdout.to_text()?)?
    print(output.stderr.to_text()?)?
    print(output.status.success(), output.status.code())?
    Ok(())
```
```output
QUIET WORDS
mode: loud

False Option[i32].Some(3)
```

`env` sets a variable for this child only. `communicate` writes the input,
reads both outputs as they arrive so that no pipe fills up, and returns the
exit status. A nonzero exit code is a normal result: the program ran. Never
put text you did not write into a `shell` script; pass it as an argument or a
variable.

A program that cannot be started is an error, and a child that runs too long
can be stopped:

```plenty
import std.process
import std.time

def main() -> Result[(), Failure]:
    mut missing = std.process.command("plenty-no-such-program")?
    match missing.spawn():
        case Ok(_):
            print("started")?
        case Err(error):
            print(error)?
    mut sleeper = std.process.command("sleep")?
    sleeper.arg("60")?
    mut child = sleeper.spawn()?
    match child.wait_timeout(&std.time.milliseconds(50u32))?:
        case Some(_):
            print("ended")?
        case Nothing:
            print("still running")?
    child.kill()?
    status = child.wait()?
    print(status.success(), status.code(), status.signal())?
    Ok(())
```
```output
IoError.System(2)
still running
False Option[i32].Nothing Option[i32].Some(9)
```

`System(2)` is the operating system's "no such file". `wait_timeout` gives up
after 50 milliseconds and leaves the child running. `kill` stops it, and `wait`
collects its status: ended by signal 9, with no exit code. Always wait for a
child you started, even after killing it; a dropped `Child` is not cleaned up.

See the references for [`std.env`](../../design/stdlib/env.md) and
[`std.process`](../../design/stdlib/process.md), including pipes you read and
write yourself.
