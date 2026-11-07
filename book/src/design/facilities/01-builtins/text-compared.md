# Plenty Text Compared to Python, Rust, and C

<div class="text-comparison">

Plenty's `str` combines **Python-like immutable values with shared ownership of
UTF-8 storage**. It is closer to an automatically cloned Rust `Arc<str>` than to
Rust's `String` or borrowed `&str`, with additional inline storage for tiny strings.
This is an analogy: Plenty exposes one string type and performs storage retention
automatically, whereas Rust makes reference-counted handle cloning explicit.

Here, Rust `Cow` means `Cow<'a, str>`, and “C string” means a conventional
NUL-terminated buffer accessed through `char *`. Costs describe the operations
before compiler optimizations that can eliminate unnecessary copies or retains.
On narrow screens, scroll each table horizontally to read all its columns.

## Ownership and assignment

<div class="text-comparison-table" role="region" aria-label="String ownership and assignment comparison" tabindex="0">

| Type | Ownership | What does `b = a` do? | How do you keep both usable? | What keeps the text alive? |
| --- | --- | --- | --- | --- |
| **Plenty `str`** | Owns its value; large strings share immutable storage | Copies the handle; retains shared storage | Ordinary assignment | Automatic reference counting for heap strings |
| **Python `str`** | Names refer to immutable string objects | Binds another name to the same object | Ordinary assignment | Python's memory management |
| **Rust `String`** | Exclusively owns a growable buffer | Moves ownership; `a` becomes unavailable | `a.clone()` duplicates the text buffer | The owning `String`; dropping it frees the buffer |
| **Rust `&str`** | Borrows text owned elsewhere | Copies the pointer and byte length | Ordinary assignment | An external owner; the borrow cannot outlive it |
| **Rust `Cow<str>`** | Either borrowed `&str` or owned `String` | Moves the `Cow` | `.clone()` copies a borrowed handle, or duplicates owned text | External owner in borrowed mode; itself in owned mode |
| **Rust `Rc<String>`** | Shared ownership of a `String` | Moves the `Rc` | `Rc::clone(&a)` copies the handle and increments its count | The last strong `Rc` owner |
| **C `char *`** | Pointer carries no ownership policy | Copies the pointer; arrays themselves are not assignable | Pointer assignment shares access; independent text needs an explicit copy | Storage duration or manual allocation/free |

</div>

Python assignment is specified as name binding; its memory-management mechanism
depends on the implementation. Rust distinguishes moving, borrowing, and cloning
explicitly. See [Python assignment](https://docs.python.org/3/reference/simple_stmts.html#assignment-statements),
[Rust `String`](https://doc.rust-lang.org/std/string/struct.String.html),
[`str`](https://doc.rust-lang.org/std/primitive.str.html),
[`Cow`](https://doc.rust-lang.org/std/borrow/enum.Cow.html), and
[`Rc`](https://doc.rust-lang.org/std/rc/struct.Rc.html).

## Mutation, construction, and views

<div class="text-comparison-table" role="region" aria-label="String mutation and views comparison" tabindex="0">

| Type | Can the existing text change? | What happens when editing or appending? | Substring without copying text? | Effect on another identifier sharing the text |
| --- | --- | --- | --- | --- |
| **Plenty `str`** | No | Builds a resulting string; allocation is fallible | Yes: borrowed `&substr` | It keeps seeing the original text |
| **Python `str`** | No | Produces a resulting string; implementations may optimize construction | Ordinary slicing generally copies; no built-in borrowed string view | It keeps seeing the original text |
| **Rust `String`** | Yes, with exclusive mutable access | Can reuse capacity; growth may reallocate | Yes: borrowed `&str` | Shared borrows prevent mutation while they remain in use |
| **Rust `&str`** | Not through this shared reference | Construct an owned string to edit independently | Yes: another borrowed `&str` | Safe Rust prevents conflicting mutation |
| **Rust `Cow<str>`** | Its owned contents can change | `to_mut()` copies borrowed text into a `String`; already-owned text can be edited directly | Yes: borrow an `&str` | Editing a borrowed `Cow` leaves its source unchanged |
| **Rust `Rc<String>`** | With unique ownership | `get_mut()` requires uniqueness; `make_mut()` clones the string when another strong owner exists | Yes: borrowed `&str` | `make_mut()` preserves the other owner's original text |
| **C `char *`** | Yes, if the underlying storage is writable | Overwrite within available capacity; manage growth manually | A pointer can identify a suffix; arbitrary substrings need a length or a terminator | Writes are visible through aliases; dangling pointers are possible |

</div>

Rust also has `&mut str`: it permits edits that preserve UTF-8 and the slice's
byte length, such as ASCII case conversion. Plenty deliberately has no mutable
substring view. See [Rust string methods](https://doc.rust-lang.org/std/primitive.str.html),
[`Cow::to_mut`](https://doc.rust-lang.org/std/borrow/enum.Cow.html#method.to_mut),
[`Rc::make_mut`](https://doc.rust-lang.org/std/rc/struct.Rc.html#method.make_mut), and
[Plenty substring views](../../language/04-expressions/slices.md#substring-views).
For `Rc::get_mut`, uniqueness excludes weak references too; `make_mut` can
instead dissociate weak references when there are no other strong owners.

Rebinding and mutation are different operations. If a mutable Plenty binding
holds a large string and another binding copies it, both retain the same text.
Assigning a different string to the mutable binding releases its old ownership
and stores the new value; it does not copy or modify the old text. The other
binding keeps the original text alive. A replacement operation such as
`text.replace(old, new)?` constructs its result before assignment updates the
binding. See [text building](text-building.md) and
[bindings, moves, and copies](../../language/05-ownership/01-bindings-moves-and-copies.md).

## Encoding, length, and indexing

<div class="text-comparison-table" role="region" aria-label="String encoding and indexing comparison" tabindex="0">

| Type | Encoding / contents | Meaning of length | Position-based access | Embedded NUL |
| --- | --- | --- | --- | --- |
| **Plenty `str`** | Valid UTF-8; Unicode scalar values | `len(s)` counts scalars; `byte_len()` counts bytes | Scalar indexing; finding a position can require scanning | Supported |
| **Python `str`** | Unicode code points; lone surrogates are representable | Code points | Code-point indexing; CPython provides constant-time access | Supported |
| **Rust `String` / `&str` / `Cow<str>` / `Rc<String>`** | Valid UTF-8; Unicode scalar values | `.len()` counts bytes; `.chars().count()` counts scalars | No integer character indexing; slicing uses byte offsets at UTF-8 boundaries | Supported |
| **C string** | Bytes; encoding is an external convention | `strlen()` scans bytes to the first NUL | Direct byte indexing; no Unicode-boundary checks | NUL ends the string |

</div>

None of these basic length operations counts user-perceived characters such as
an emoji sequence or a letter plus combining marks. That requires grapheme
segmentation. See [Plenty text](text.md),
[segmentation and display width](../../stdlib/text.md#segmentation-and-display-width),
[Python strings](https://docs.python.org/3/library/stdtypes.html#text-sequence-type-str),
[CPython representation](https://docs.python.org/3/c-api/unicode.html), and
[Rust UTF-8 representation](https://doc.rust-lang.org/std/string/struct.String.html#representation).
For C's byte-string and storage contracts, see ISO C's
[public C11 committee draft](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1570.pdf),
sections 6.4.5, 6.5.16, 7.22.3, and 7.24.

## Consequences for Plenty's design

- **“Copy” has a broader meaning than Rust's `Copy`.** Plenty can automatically
  increment a reference count. Rust's `Copy` is strictly a bitwise copy; `Rc`
  and `Arc` require explicit cloning. See the
  [Rust `Copy` contract](https://doc.rust-lang.org/std/marker/trait.Copy.html).
- **Sharing costs are independent of text size.** Assigning a large Plenty
  string copies a handle and retains storage, while cloning a Rust `String`
  duplicates its bytes. Plenty currently uses atomic reference counts for heap
  storage; inline strings and literals avoid that counting work. See
  [value representation](../../toolchain/02-architecture/values.md).
- **Immutability trades editing flexibility for easy sharing.** Plenty and
  Python make assignment and retaining old versions straightforward. Rust
  `String` supports efficient repeated edits through exclusive ownership.
- **Plenty separates construction from the finished value.**
  [`std.text.Builder`](../../stdlib/text.md) provides mutable construction;
  the resulting `str` is immutable. Strings of up to seven UTF-8 bytes fit
  directly inside Plenty's value word; see [text storage](text.md).

</div>
