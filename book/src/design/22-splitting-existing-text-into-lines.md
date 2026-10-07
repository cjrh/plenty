# Splitting existing text into lines

`str.splitlines(keepends: bool = False) -> Result[list[str], AllocError]`
returns independently owned lines. It recognizes LF, CR, CRLF, vertical tab,
form feed, U+001C–U+001E, U+0085, U+2028, and U+2029. Empty text produces no
lines, interior blank lines are retained, and a final terminator adds no extra
empty line. `True` keeps original terminators without translating them. This is
deliberately broader than file universal-newline decoding (CR/LF only).
Two borrowed scans reserve list storage once; every output string is fallible,
and any failed construction releases the initialized prefix.
