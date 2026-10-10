# Text and file examples

Use these examples to look up text methods and advanced file operations.
The [text I/O reference](../../facilities/02-io/index.md)
lists signatures and platform limits.

Text methods borrow their inputs. Queries that return scalars or `Option`
allocate nothing; operations producing independent strings or collections
report allocation failure in a `Result`.
