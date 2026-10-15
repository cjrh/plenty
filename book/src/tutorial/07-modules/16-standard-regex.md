# Match text with regular expressions

`std.regex` compiles a pattern once into a `Regex`, then searches text with
it. Matching takes time proportional to the length of the text, whatever the
pattern: there is no backtracking, so no pattern can make a search run for
hours. `compile` takes `Limits`, which also bound the pattern itself, so a
pattern from an untrusted source is safe to compile.

## Find matches

`find` returns the first match as byte offsets; `text` copies the matched
part. `find_all` returns every match and fails rather than return more than
its limit:

```plenty
import std.regex

def main() -> Result[(), Failure]:
    limits = std.regex.limits()
    year = std.regex.compile("\\b(19|20)\\d\\d\\b", &limits)?
    text = "Released 1999, revised 2024, not 12345."
    match year.find(text)?:
        case Some(found):
            print(found.start, found.end, found.text(text)?)?
        case Nothing:
            print("no year")?
    print(len(year.find_all(text, 10)?))?
    print(year.is_match("in 1850")?)?
    print(str.repr(year.find_all(text, 1))?)?
    Ok(())
```
```output
9 13 1999
2
False
Result[list[std.regex.Match], std.regex.RegexError].Err(std.regex.RegexError.LimitExceeded(std.regex.Limit.Matches))
```

A Plenty string needs `\\` for each backslash in the pattern. `\d`, `\w`,
`\s` and `\b` are ASCII-only; for other scripts, use properties such as
`\p{Alphabetic}`.

## Groups and replacement

Parentheses capture. `(?P<name>...)` names a group. `captures` returns every
group of the first match; a group that did not take part is `Nothing`. In a
replacement template, `$1` and `${name}` insert a group and `$$` inserts a
dollar sign:

```plenty
import std.regex

def main() -> Result[(), Failure]:
    limits = std.regex.limits()
    date = std.regex.compile("(?P<year>\\d{4})-(?P<month>\\d{2})", &limits)?
    text = "from 2024-05 to 1999-12"
    match date.captures(text)?:
        case Some(groups):
            print(groups.name("year").unwrap().text(text)?)?
            print(groups.get(2).unwrap().text(text)?)?
        case Nothing:
            pass
    print(date.replace_all(text, "${month}/$1", 100)?)?
    words = std.regex.compile("\\s*,\\s*", &limits)?
    print(" | ".join(words.split("red , green,blue", 10)?)?)?
    Ok(())
```
```output
2024
05
from 05/2024 to 12/1999
red | green | blue
```

The last argument of `replace_all` and `split` is a limit on the result size
or the number of pieces. Write `${1}0` when a group number is followed by a
digit. A bare `$name` is an error: use `${name}`.

## Errors and limits

A pattern error reports its byte offset. A pattern that would build too large
a matcher, and a search that would do too much work, are limit errors:

```plenty
import std.regex
from std.regex import Limits

def main() -> Result[(), Failure]:
    limits = std.regex.limits()
    print(str.repr(std.regex.compile("total: (\\d+", &limits))?)?
    print(str.repr(std.regex.compile("(a{1000}){1000}", &limits))?)?
    tight = Limits(1000, 16, 100, 1000, 1000000, 100)
    word = std.regex.compile("\\w+!", &tight)?
    print(str.repr(word.find("no exclamation mark in this sentence"))?)?
    Ok(())
```
```output
Result[std.regex.Regex, std.regex.RegexError].Err(std.regex.RegexError.Syntax(std.regex.SyntaxError(offset=7, message="unclosed group")))
Result[std.regex.Regex, std.regex.RegexError].Err(std.regex.RegexError.LimitExceeded(std.regex.Limit.Instructions))
Result[Option[std.regex.Match], std.regex.RegexError].Err(std.regex.RegexError.LimitExceeded(std.regex.Limit.Work))
```

The default limits suit patterns written into a program. For patterns from
users, lower them to what a request may spend.

See the [`std.regex` reference](../../design/stdlib/regex.md) for the full
syntax, every limit, and searching bytes.
