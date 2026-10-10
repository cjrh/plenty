# Functions as values

Follow this optional path when code should accept an operation chosen by its
caller, or keep an operation for later. Begin with named functions, then use
anonymous functions and captures for ordinary local work. Borrowed and stored
environments appear when their ownership needs arise; consuming jobs come last.

The first lessons need only the core path. Lessons about generic signatures and
records also use [type parameters and constraints](../09-generics/index.md).
You can return to those API-design lessons after writing local callbacks.

- [Pass functions as values](named-callbacks.md)
- [Multiline anonymous functions](anonymous-functions.md)
- [Capture data and private state](owned-captures.md)
- [Borrow in a closure](borrowed-captures.md)
- [Return a closure](returning-closures.md)
- [Generic function values](generic-function-values.md)
- [Accept functions and closures through one API](generic-callback-apis.md)
- [Transform a cell with a generic method](generic-method-callbacks.md)
- [Store stateful callbacks](explicit-state-callbacks.md)
- [Store owned closures](callback-records.md)
- [Keep callbacks in tuples and lists](callback-collections.md)
- [Name stored callbacks](named-callback-registry.md)
- [Put a callback in an enum](callback-enums.md)
- [Consume a capture](consuming-captures.md)
- [Consume a callback](consuming-callback-apis.md)
- [Consume owned data while borrowing local state](consuming-borrowed-captures.md)
- [Store consuming jobs](stored-consuming-jobs.md)
