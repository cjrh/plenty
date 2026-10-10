# Text and file examples

These supplementary examples exercise detailed contracts outside the core
tutorial. Start with the [text I/O reference](../../05-current-language-contract/01-practical-text-i-o.md)
for signatures and platform limits. The core learning path teaches whole files,
`open` with `with`, `read`, `write`, and a `readline` loop.

Text methods borrow their inputs. Queries that return scalars or `Option`
allocate nothing; operations producing independent strings or collections
report allocation failure in a `Result`. The individual examples spell out
boundaries and Unicode behavior where these affect callers.
