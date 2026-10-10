# Tuple operations

Tuples provide fixed component access with a nonnegative integer literal index.
Construction, indexing, unpacking, and component-loan rules are defined in
[tuple types](../../language/03-types/tuples.md).

Supported tuples compare structurally without allocation. [copy](construction-copy.md)
duplicates copyable components and mutable contents fallibly;
[formatting](formatting.md) displays supported component values. Types containing
stored recursive data are excluded from these automatic traversals.

A tuple can store concrete owned callbacks of different types. Reusable calls
borrow their indexed component; consuming calls require extraction by unpacking.
See [stored callable values](../../language/06-functions/index.md).
