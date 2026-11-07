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

On any line of a top-level `def`, `class`, `enum`, `protocol`, `const`, or
`type`, including its decorators, docstring, and body and the comment lines
attached to it, the server offers one
**Move `NAME` to module `MODULE`** action for each other module of the
package's `src/`. Choosing one resolves the action into a workspace edit; an
editor that previews edits shows every file before applying any. The edit is
computed from the unsaved text of open files, and an action offered before
the file changed is refused; ask for code actions again.

The move:

- carries the declaration with its decorators, its docstring, and the comment
  lines at the left margin directly above it or above its first decorator,
  with no blank line between; a comment after a blank line, or after the
  body, belongs to the next declaration. The declaration is appended to the
  destination after a blank line;
- copies the imports the moved code uses, and removes them from the source
  when nothing else there uses them;
- imports the source module's declarations that the moved code still uses,
  and names the destination's own declarations directly. When the module
  that now needs a declaration already has a plain `import` of its module,
  the name is qualified (`colors.name`) instead of adding a `from` import;
  a new `from` import joins one of the same module already there, in order
  when its names are sorted;
- rewrites every caller in the package's `src/` and `tests/` files, and in
  workspace members that depend on it: `from` imports, aliases, and qualified
  names (`shapes.Point` becomes `geometry.Point`), adding or removing `import`
  lines as needed;
- adds `pub` to the moved declaration, or to a source declaration it uses,
  when another module now needs it;
- writes the destination's line endings, `\r\n` or `\n`;
- moving into the package's library (`src/lib.plenty`) writes imports of it
  from `tests/` and other packages as the package name (`from tools import
  Point`). Moving out of the library is refused, since code outside `src/`
  imports its declarations by the package name.

A source module left with only imports and comments stays in place, and the
compiler then warns that no target imports it; delete it if it is no longer
needed.

Changes to a package's API are listed with the edit: a declaration made
public; a public library declaration whose import path changes, since
packages outside the workspace that import it must change; and a moved
`class` or `enum`, whose printed name includes its module: printing a moved
`Point` writes `geometry.Point(x=1, y=2)` instead of `shapes.Point(x=1, y=2)`,
and a type from a dependency keeps its package prefix
(`tools::geometry.Point(x=5)`). When the client
supports change annotations, these edits carry an annotation that needs
confirmation; otherwise the server shows the changes in a warning message.

Before answering, the server checks the package without and then with the
edits: each library, executable, and test file of the package and of the
workspace members that depend on it is checked as an entry, and then each
`src/` file no entry reached. Each entry checks the modules it imports,
including `std` modules, again; after the edits, an entry that reaches no
edited file is not checked again. The cost grows with the number of entries:
with a test file that imports the library and `std.test`, a move took about
0.07 s with no tests, 1 s with 10, 3.9 s with 40, and 16 s with 160, on a
release build. The check after the edits catches what the rewriting does not
detect itself, such as an import binding the destination already uses for
something else.

A move is refused, with nothing edited and the reason returned as a request
error, when:

- the file is not in a package, or the destination is not another module in
  the same `src/` directory;
- the line is not a supported declaration: `extern`, `opaque`, `dimension`, and
  `if const` blocks are refused;
- the declaration is `main` in a target's entry file, which runs it;
- the moved text contains an [`if const`](../../language/02-source/target-conditionals.md)
  block, or an `if const` block in a file the check reaches names the moved
  declaration, a `from` import alias of it, the source module's import
  binding in that file, the source module's last path component, or a
  binding of an import line the move adds, removes or rewrites in that file.
  Unselected arms are not checked on the current target, so the move could
  neither rewrite nor check them. The match is a whole-word text match over
  every arm of the block, including the selected arm, comments and strings,
  so it can refuse a move that no arm needs;
- the source is the package's library;
- the declaration uses `include_str` or `include_bytes`, whose paths are
  relative to its file's directory, and the destination is in another
  directory;
- the package does not check before the move;
- the destination already declares or imports the name, or an import binding
  would conflict;
- an import that must be rewritten carries a comment;
- an import the move adds would close a circular import through an import
  inside an `if const` block. Imports inside `if const` blocks are never
  rewritten; they refuse a move only through this rule and the `if const`
  rule above;
- a use of the declaration has a form the move cannot rewrite, such as
  `shapes . Point` with spaces or a member access split across lines;
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
  implemented. Moving a declaration is the only refactoring. It checks each
  entry twice, so a package with many test files takes seconds; see
  [moving a declaration](#moving-a-declaration-to-another-module).
- Moving a declaration and then moving it back does not always restore the
  text. The declaration returns to the end of its module, and stays `pub` if
  the first move made it public; an import alias the first move removed, or
  a qualified name it made bare, is not restored. Moving it there and back
  again then changes nothing.
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
