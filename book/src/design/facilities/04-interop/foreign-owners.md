# Owning foreign resources

A public class can own a private opaque pointer and invoke its matching foreign
destroy function in `__del__`. It moves, cannot be copied, and cleans up on
ordinary scope exits and error propagation. Borrowing the class retains native
ownership. The binding chooses shared or exclusive methods from the native
library's actual mutation and aliasing guarantees, not merely its use of const.

For fallible acquisition, construct an owner containing null first, then acquire
into its pointer inside a Result-returning factory. Its destructor is active
before any native resource exists, so partial acquisition or subsequent failure
still cleans up. Class storage is inline and requires no allocation; allocation
explicitly requested by fields or native acquisition retains its own contract.

The destructor guards null unless the native destroy operation accepts it, and
uses the matching native release function rather than Plenty's allocator.
Native allocation failure is recoverable only if the library exposes such an
error.

## Transfer policies

- If C consumes unconditionally, move the owner into the wrapper and clear its
  pointer before calling C, on both success and failure paths.
- If C consumes only on success, clear the pointer on success and return the
  intact owner on failure, for example as `Err(owner)` in `Result[(), Handle]`.
- Borrowed calls retain ownership and expose `&Handle` or `&mut Handle` according
  to the permitted access. C must not retain borrowed addresses.

Raw declarations cannot prove these policies. The wrapper and native implementation
must agree, including partial effects, null handling, and thread restrictions.
For Plenty objects exported to C, use the [generated handle contract](owned-exports.md).
