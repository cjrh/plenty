# Defining your own data

Use classes for records with fields and methods, and enums for alternatives.
Their fields follow the same ownership and cleanup rules as collections.

[Borrowed cursors](user-iterators.md) combine a view class with an exclusive
`next` method to lend one item at a time.

[Heterogeneous collections](../heterogeneous-collections/index.md) combines
classes and enums in lists and dictionary values, and explains the restrictions
on sets and dictionary keys. [Special-purpose data structures](../special-purpose/index.md)
introduces columnar lists for scanning selected primitive fields.
