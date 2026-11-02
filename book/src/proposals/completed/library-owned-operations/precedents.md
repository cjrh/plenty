# C++ and Rust precedents

**Status: investigation evidence.** Supports the
[library-owned operations assessment](../library-owned-operations.md), not a
change to Plenty's protocol semantics. Sources and probes checked 2026-10-17.

## C++: override and call access are separate

The library author can write:

```cpp
class Transport {
public:
    void send() { validate(); send_impl(); record_success(); }
    virtual ~Transport() = default;
private:
    virtual void send_impl() = 0;
    void validate();
    void record_success();
};
```

The backend author overrides `send_impl` privately in a derived class. An
application author calls `send` through `Transport&`. The wrapper statically
selects the library's algorithm, which dynamically invokes the backend hook.
Access does not participate in deciding whether one virtual function overrides
another; call accessibility is checked separately using the static expression
type. [C++ working draft: virtual functions](https://eel.is/c++draft/class.virtual),
[access to virtual functions](https://eel.is/c++draft/class.access.virt).

A derived class can declare another `send` that hides the base name, or expose
another public operation. Calling through the derived type can therefore select
another API; NVI does not prevent that. The guarantee concerns calls through the
base abstraction. In the focused probe, counters showed that `Transport&.send`
used the wrapper and the private override, while `Memory.send` selected the
derived hiding method. Direct calls to the private hook through either type were
rejected. The probe compiled and ran with GCC 16.2.1, C++20 and
`-Wall -Wextra -Werror`.

The issue's [Standard C++ FAQ link](https://isocpp.org/wiki/faq/strange-inheritance/1000)
returned HTTP 403 during this investigation; the accessible working draft and
compiled probe supplied the evidence instead. Exceptions/partial external effects
still need a library-specific contract; this minimal example omits them.

## Rust: composition supplies the fixed operation

The [Rust Reference on traits](https://doc.rust-lang.org/reference/items/traits.html)
specifies replaceable defaults and rejects visibility annotations on individual
trait items. A public trait with default `send` and required `send_impl` therefore
does not express either hidden hook access or a fixed algorithm. This is a
constraint on that spelling, not on Rust's ability to express the intent.

A library author can instead write this public surface, with private fields:

```rust
pub struct Permit { _private: () }
pub trait Backend {
    fn send_impl(&mut self, message: &str, permit: &Permit) -> Result<(), ()>;
}
pub struct Transport<B> { backend: B, sent: usize }

impl<B: Backend> Transport<B> {
    pub fn send(&mut self, message: &str) -> Result<(), ()> {
        if message.len() > 4 { return Err(()); }
        self.backend.send_impl(message, &Permit { _private: () })?;
        self.sent += 1;
        Ok(())
    }
}
```

A public constructor implemented in the library moves `B` into the wrapper.
An external backend author implements `Backend` and names `Permit`, without
creating it. An application author obtains `Transport<B>` and calls its inherent
`send`. This excerpt isolates the mechanism; the complete Plenty example also
specifies connection state and poisoning after backend failure.

Without the token, a separate backend value's trait hook is callable. With it,
the hook still has public visibility, but callers need a value they cannot
construct. `Permit` exposes no constructor, `Default`, `Copy` or `Clone`.
The borrowed permit cannot be retained as an owned token by safe code; storing
its reference requires a compatible lifetime that a callback-local permit does
not provide. This does not restrict external trait implementations. Rust's
[privacy rules](https://doc.rust-lang.org/reference/visibility-and-privacy.html)
make public trait items accessible but preserve private struct fields.

This adds a wrapper, inherent implementation/bound, delegation and one public
token type/parameter. Separate crates need the extension trait and its argument
types exposed as public contracts. A private implementation trait is simpler
when *all* backends belong to the library, but removes external extensibility.
Documentation hiding alone supplies no call restriction.

## Sealing does not hide callable hooks

The [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/future-proofing.html#sealed-traits-protect-against-downstream-implementations-c-sealed)
describe sealing to restrict downstream implementations. That is a different
permission from calling a hook. In a separate-crate probe, the library put
`pub trait Hook` in a private module, made public `Api` inherit it, and implemented
both for `Concrete`. An application still compiled this generic call:

```rust
fn bypass<T: Api>(value: &T) -> i32 { value.hook() }
```

The application did not need to name the hidden supertrait. Bounds provide
access to supertrait associated items, as the Rust Reference explains. Thus
neither a private module path nor sealing is proof of hidden call access.

## Rust probe outcomes

Separate library, backend and application crates compiled with rustc 1.99.0,
edition 2021 and `-D warnings`; the application verified one successful send,
size rejection, backend error and a count of one. No unsafe code was used.

| Focused probe | Result |
| --- | --- |
| External backend implements token-bearing trait | Accepted |
| Application constructs private-field token | E0451 |
| Application calls hook without token | E0061 |
| Backend tries to clone `&Permit` into owned `Permit` | E0308: reference cloned; token does not implement `Clone` |
| Visibility annotation on trait method | E0449 |
| Implementor replaces a default `send` | Accepted; executed replacement returned the alternative value |
| Application's generic bound calls private-module supertrait hook | Accepted and ran |

These small probes validate the language mechanisms, not transport reliability
or database transaction semantics. They support using composition as the baseline
and capability possession as a separate design choice.
