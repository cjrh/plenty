# Language server

`plenty lsp` serves the [Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
on standard input and output. It checks files with the compiler's own front
end, so its errors and names match `plenty check` and `plentyc --check`. Neovim
is the supported editor; see [Neovim support](neovim.md) for its setup.

## Features

| Request | Behavior |
|---|---|
| Diagnostics | The file's check errors, published when it is opened, saved, or edited (after a 150 ms pause) |
| Hover | The declaration as written and its docstring, as Markdown |
| Go to definition | The declaration's name, in the same file, another module, a dependency, or `std` |
| Inlay hints | Inferred types after unannotated simple local binding names, when the current check succeeds |
| Completion | Names in scope and keywords; fields, methods, enum variants, and module members after `.` |

Hover and definition work on functions, methods, classes, enums, enum
variants, protocols, fields, constants, type aliases, imported names, modules
in an `import`, locals, and parameters. A hover shows a function's full header,
including type parameters, the receiver (`self`, `&mut self`), defaults, and
the return type, and `pub` when it is written. Hover on a local or parameter
shows its type as the checker inferred it. A method call resolves through its
receiver's type, so `p.dist()` opens the method of `p`'s class. Completion
items carry the declaration and the docstring's first line.

A docstring is the string that begins a function body; see
[signatures and calls](../../language/06-functions/01-signatures-and-calls.md).
Classes, enums, and fields have no docstrings.

## Inferred types on code lines

Enable **inlay hints** in your editor to see, for example, `: i64` after the
name in `count = 42`. These are editor-rendered annotations, not source edits;
you can keep writing inferred bindings without inserting type annotations.
Other language servers provide inferred type annotations through this same
[LSP 3.17 inlay-hint request](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/#textDocument_inlayHint).
The editor requests hints for a document range and decides how to display them.
See [Neovim setup](neovim.md#inferred-type-hints) to enable them.

Plenty supplies type hints for new simple local bindings, including `mut`,
using the checker's type spelling as hover does. Explicit annotations,
reassignments, parameters, tuple destructuring, loop and pattern bindings do
not get hints. Compiler-internal and producer-specific types (such as local
pipelines and capturing closures) are omitted. An uninstantiated generic body has no
checked hints; when instantiations disagree on a binding's type, it is omitted.
Hints are withheld while the current check has errors, rather than guessing
or displaying types and positions from an older version of the source.

The server advertises `inlayHintProvider` with no resolve support and returns
complete string labels of kind `Type`, without text edits or extra padding.
Hint positions are immediately after the binding name, within the requested
half-open range, using the negotiated UTF-8, UTF-16 (default), or UTF-32
position encoding. Hint requests reuse the document's analysis; a pending edit
is checked first, just as for hover. No parameter-name hints are provided.

## How a file is checked

The server checks the file being edited as the entry of a compilation, with
the unsaved text of every open file in place of its saved text. A file that
defines `main` is checked as an application, and any other file as a library
module, as `plentyc --check-module` does. Inside a package (a `plenty.toml`
above the file's `src/`), imports resolve through the package graph without
fetching or changing `plenty.lock`; run `plenty fetch` once for Git
dependencies. Outside a package, imports resolve from the file's directory.

Checking recovers from an error in one function: the function keeps its
signature, the other functions are still checked, and each error is
published. Errors that stop a whole module, such as a syntax error, an
unresolved import, or an invalid class declaration, are published alone;
while one is present, hover, definition, and completion use the names found by the file's last check
that got further.

Definitions in an embedded `std` module open as a read-only copy of its
source, written to `std-sources/VERSION/std/` below `$PLENTY_CACHE_DIR`,
`$XDG_CACHE_HOME/plenty`, or `~/.cache/plenty`. The copies are ordinary files,
so any editor can open them, and they are rewritten when the compiler's copy
differs.

## Limits

- Names inside a generic function or method are resolved only where it is
  instantiated with concrete types; a method called on a protocol-bounded
  parameter opens the method of the class it was instantiated with, not the
  protocol's requirement. A method of a generic class is instantiated only
  where it is used. An uninstantiated generic body resolves its module-level
  names and locals only.
- Builtin functions, methods of built-in types such as `list` and `str`, and
  `Ok`, `Err`, `Some`, and `Nothing` have no declaration to show.
- Variant names in `case` patterns are not resolved.
- Locals offered by completion are those bound in the enclosing function
  above the cursor, by line.
- An error in a module other than the open file is shown on the file's first
  line, with the other file's position in the message.
- Each check reads the whole program again; there is no incremental reuse
  between checks. A program importing `std.http.server`, `std.json`, and five
  other large `std` modules checks in about 0.2 s with a release build.
- Find references, document symbols, and signature help are not implemented.
- A message body that is not valid JSON is answered with a `-32700` parse
  error and the session continues. A header without a valid `Content-Length`
  leaves the stream unframed, so the server exits as if the input had ended.
- After `shutdown`, the server answers any other request with a `-32600`
  invalid-request error and ignores notifications until `exit`.

## Other editors

Any client that starts a server with a command line and speaks LSP over stdio
can use `plenty lsp` for files with the `.plenty` extension. No VS Code
extension is provided; a generic LSP client extension configured with the
command `plenty lsp` works.
