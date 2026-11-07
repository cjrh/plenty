# Language server

`plenty lsp` serves the [Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
on standard input and output. It checks files with the compiler's own front
end, so its errors and names match `plenty check` and `plentyc --check`. Neovim
is the supported editor; see [Neovim support](neovim.md) for its setup.

## Features

| Request | Behavior |
|---|---|
| Diagnostics | The file's check errors and [warnings](warnings.md), published when it is opened, saved, or edited (after a 150 ms pause) |
| Hover | The declaration as written and its docstring, as Markdown |
| Go to definition | The declaration's name, in the same file, another module, a dependency, or `std` |
| Inlay hints | Inferred types after unannotated simple local binding names, when the current check succeeds |
| Completion | Names in scope and keywords; fields, methods, enum variants, and module members after `.` |
| Code actions | `refactor.move`: move a top-level declaration to another module of the package, previewed before it is applied |

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

## Moving a declaration to another module

On the line of a top-level `def`, `class`, `enum`, `protocol`, `const`, or
`type`, the server offers one **Move `NAME` to module `MODULE`** action for
each other module in the same `src/` directory. Choosing one resolves the
action into a workspace edit; an editor that previews edits shows every file
before applying any. The edit is computed from the unsaved text of open files.

The move:

- carries the declaration's docstring and the comment lines
  directly above it, and appends it to the destination after a blank line;
- copies the imports the moved code uses, and removes them from the source
  when nothing else there uses them;
- imports the source module's declarations that the moved code still uses,
  and names the destination's own declarations directly;
- rewrites every caller in the package's `src/` and `tests/` files, and in
  workspace members that depend on it: `from` imports, aliases, and qualified
  names (`shapes.Point` becomes `geometry.Point`), adding or removing `import`
  lines as needed;
- adds `pub` to the moved declaration, or to a source declaration it uses,
  when another module now needs it.

Changes to a package's API are listed with the edit: a declaration made
public, and a public library declaration whose import path changes, since
packages outside the workspace that import it must change. When the client
supports change annotations, these edits carry an annotation that needs
confirmation; otherwise the server shows the changes in a warning message.

Before answering, the server checks the whole package with the edits applied.
A move is refused, with nothing edited and the reason returned as a request
error, when:

- the file is not in a package, or the destination is not another module in
  the same `src/` directory;
- the line is not a supported declaration: `extern`, `opaque`, `dimension`, and
  `if const` blocks are refused;
- the package does not check before the move;
- the destination already declares or imports the name, or an import binding
  would conflict;
- an import that must be rewritten carries a comment, or a touched file has an
  indented import;
- a use of the declaration has a form the move cannot rewrite;
- the moved code needs the source module and the source needs the moved code,
  so the two modules would import each other;
- the package would not check after the move, for example because of a
  circular import or a private name the destination cannot reach;
- a file reached by a target or test before the move would no longer be
  reached, unless it is left with only imports and comments.

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
- Find references, rename, document symbols, and signature help are not
  implemented. Moving a declaration is the only refactoring; it checks the
  package twice, so it takes about as long as two checks of every file.
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
