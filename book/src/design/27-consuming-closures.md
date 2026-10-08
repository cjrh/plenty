# Consuming closures

`def once [values](...) -> T:` creates an affine, one-shot closure. Construction
moves owned captures into an inline environment, exactly as for reusable closures.
Calling it consumes the environment and transfers captured owners into ordinary
body parameters. The body can return or otherwise move those owners. `mut value`
permits changing a captured value during that call. The closure binding itself
does not need `mut`; it is consumed, not borrowed.

Calling twice, calling through a reference, or passing it to a reusable `Callable`
constraint is rejected. A capture-free `def once (...)` is still affine. Captured
names cannot be shadowed. Borrowed captures are currently rejected for one-shot
closures. Reference returns, generators, and heap storage retain the limits of
[reusable environments](26-closures.md).

Uncalled environments drop their captures in reverse order. When called, the body
owns cleanup: captures that are moved into the result remain alive, and all other
captures drop on exit. Argument failure before entry drops the pending environment.
There is no heap allocation for creating, moving, calling, or dropping the closure.
