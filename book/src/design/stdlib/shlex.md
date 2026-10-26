# std.shlex

`std.shlex` splits text containing shell-style words and quotes one argument
for a POSIX shell. It is written in Plenty and never starts a process, reads
the environment, or expands shell expressions.

Prefer [`std.process.Command.arg`](process.md#building-a-command) when you
already have individual arguments: pass them directly, without quoting.
Use `split` for an argument string supplied by a configuration file or user.

## Interface

| Function | Result | Contract |
|---|---|---|
| `split(text: &str, comments: bool = False)` | `Result[list[str], ShlexError \| AllocError]` | Remove word quoting and escapes, returning owned arguments |
| `quote(text: &str)` | `Result[str, ShlexError \| AllocError]` | Encode one argument for a POSIX shell |

```plenty
import std.shlex
from std.shlex import ShlexError

def main() -> Result[(), ShlexError | AllocError | IoError]:
    print(std.shlex.split("--name 'Ada Lovelace' --literal '$HOME'")?)?
    print(std.shlex.quote("two words")?)?
    Ok(())
```
```output
["--name", "Ada Lovelace", "--literal", "$HOME"]
'two words'
```

## Splitting rules

- Space, tab, carriage return and line feed separate unquoted words. Other
  Unicode whitespace is ordinary data; this whitespace convention follows
  Python's `shlex` rather than a shell's command grammar.
- Single quotes preserve everything until the next single quote. Double
  quotes preserve whitespace and single quotes. Adjacent quoted and unquoted
  parts join into one word, and `''` or `""` creates an empty word.
- Outside quotes, backslash preserves the next scalar. Inside double quotes,
  it escapes only `$`, backtick, double quote, backslash and line feed;
  before anything else, the backslash is retained.
- Backslash followed by line feed is removed outside single quotes. Alone,
  that continuation does not create an empty word.
- With `comments=True`, an unquoted `#` at the start of a word skips the rest
  of that line. `a#b`, quoted hashes and escaped hashes stay literal. Comments
  are disabled by default.

There is no variable, command, arithmetic, tilde or wildcard expansion.
Operators such as `;`, `|`, `>` and `&&` are ordinary word contents, not a
pipeline or a command separator. Dollar-single-quoted strings, substitutions,
here-documents and shell grammar are not supported. This is not a shell
script validator, nor an exact implementation of Python's `shlex` modes.

## Quoting and errors

`quote` leaves nonempty ASCII words containing only letters, digits and
`_@%+=:,./-` unchanged. Otherwise it uses single quotes and escapes embedded
single quotes by closing and reopening the quoted sections. Empty text becomes
`''`. For every accepted input, `split(quote(text)?)?` returns exactly one
word equal to `text`.

Quoted text is for one argument in a noninteractive POSIX shell (`sh -c` or a
script), not for an entire script or for Windows shells. Control characters
remain literal; do not paste or feed this output to an interactive shell.
It does not make an executable name, option,
path, or shell program appropriate for an application. Prefer argument-vector
spawning; when a shell is needed, passing data through arguments or environment
variables also avoids building scripts from data.

`ShlexError` has `UnclosedQuote(position)`, `TrailingEscape(position)` and
`Nul(position)`. Positions are zero-based Unicode scalar indices, matching
Plenty string indexing. The quote/escape position points to its opening
character. Both functions reject NUL, including in a comment, because POSIX
process arguments cannot contain it. Allocation failures return `AllocError`;
no partial result is exposed and intermediate storage is released.

Splitting takes linear time and uses a reusable scalar-piece list plus the
returned words. Quoting takes linear time and may allocate intermediate text
when escaping apostrophes. These are whole-string APIs; callers handling
untrusted, arbitrarily large input should bound that input before parsing.

The API draws on [Python's `shlex`](https://docs.python.org/3/library/shlex.html)
and the [Rust `shlex` crate](https://docs.rs/shlex/2.0.1/shlex/), with the
specific splitting rules above rather than a compatibility mode switch.
