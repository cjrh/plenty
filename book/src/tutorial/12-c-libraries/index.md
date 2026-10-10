# Working with C libraries

Follow this optional path after the core lessons about modules, text,
borrowing, resource owners, and `Result`. It can be read independently of the
generic, callback, and concurrency paths.

Start by calling a trusted interface, then adapt text and own a foreign resource.
When exporting a library, make its C callers' error and ownership contracts
explicit. Runtime loading builds on those same contracts.

- [Call C libraries](calling-c.md)
- [Pass text to C](passing-text.md)
- [Own a foreign handle](foreign-resources.md)
- [Build a C library](building-a-library.md)
- [Report errors through an exported function](export-result-contracts.md)
- [Export owned objects](export-owned-objects.md)
- [Load a library at runtime](runtime-loading.md)
