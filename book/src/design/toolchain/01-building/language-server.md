# Language server

`plenty lsp` serves the [Language Server Protocol](https://microsoft.github.io/language-server-protocol/)
on standard input and output. It checks files with the compiler's own front
end, so its errors and names match `plenty check` and `plentyc --check`. A
file below a package's `tests/` directory is checked as `plenty test` checks
it, so it can import its own package by name. Neovim
is the supported editor; see [Neovim support](neovim.md) for its setup.

## Features

| Request | Behavior |
|---|---|
| Diagnostics | The file's check errors and [warnings](warnings.md), published when it is opened, saved, or edited (after a 150 ms pause) |
| Hover | The declaration as written and its docstring, as Markdown |
| Go to definition | The declaration's name, in the same file, another module, a dependency, or `std` |
| Find references | Every written name that refers to the declaration, across the package and the workspace members that depend on it; see [finding references](#finding-references) |
| Rename | A module-level declaration, a field, method, or enum variant, an import alias, or a local, across the same files as find references; checked before and after, and refused with a reason; see [renaming](#renaming) |
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

## Finding references

Find references lists every place a name refers to the same declaration as
the name under the cursor, by what the checker resolved rather than by
spelling: a parameter or local that shadows a function is a different
declaration, and a `from` import alias (`from shapes import area as size`)
refers to `area` both in the import and where `size` is used. The
declaration's own name is listed unless the client asks to exclude it.

The search covers the package holding the declaration and the workspace
members that depend on it. As when
[moving a declaration](#moving-a-declaration-to-another-module), each
library, executable, and test file of those packages is checked as an
entry, and then each `src/` file no entry reached, with the unsaved text of
open files in place of their saved text. A declaration in `std` or in a
dependency outside the workspace is searched for in the package of the file
being edited and the members that depend on it; references inside `std` and
the declaration's copy are listed only as the declaration itself. A local, a
parameter of a callable only its own module can call, and any name in a file
outside a package are answered from the file's own check. A parameter of an
exported function, or of a public method of an exported type, is searched like
a package-level declaration, since named arguments to it can be in other
modules. A package file that no entry reads, such as one under
`examples/`, adds its own check's references to the package's.

An entry that does not check still contributes the references its check
recorded. The server then shows a warning that the list may be incomplete,
with the first error. A search of a package-level declaration checks every
entry once, so in a package with many test files it takes seconds.

The list contains only names the checker resolves. Named arguments
(`area(n=2)`) refer to the parameter, constructor keywords (`Point(x=1)`) to
the field or `__init__` parameter, and variants and fields in `case` patterns
and variant constructors (`Shape.Circle(r)`) to their declaration.

A generic function or method that no call instantiates is not type-checked,
so its members are found by the types written in it. A field or method of a
parameter, `self`, or a local whose class the body states, through an
annotation, a constructor, a field, or a call's declared result, refers to
that member; a method of a protocol-bounded type parameter refers to the
protocol's requirement. A member of a value whose type the body does not
state, such as `points[0].x`, is not listed. The list also misses names in
unselected [`if const`](../../language/02-source/target-conditionals.md)
arms. Text in strings, comments, and docstrings is never a reference.

## Renaming

Rename (`textDocument/rename`, with `prepareRename`) changes a name and every
name that refers to the same declaration, as
[find references](#finding-references) finds them. It renames:

- a top-level function, class, enum, protocol, constant, or type alias, in
  the package holding it and the workspace members that depend on it;
- a field, method, or enum variant, in the same packages: its declaration,
  member accesses and calls, constructor keywords, `case` patterns, and
  f-string fields;
- a `from` import alias: renaming `size` in `from shapes import area as
  size` changes the alias and its uses in that file only;
- a parameter, with the named arguments that pass it (`area(width=2)`) and
  the constructor keywords of an `__init__` parameter: in the package and its
  dependent members for a parameter of an exported function or of a public
  method of an exported type, and in its file otherwise;
- a local variable, in its function.

Renaming `area` itself keeps an alias: the import becomes `from shapes import
measure as size`, and `size(2)` stays. A qualified use `s.area` becomes
`s.measure`. Strings, comments, and docstrings are not edited, and a module
is not renamed with its file. The edit uses the unsaved text of open files,
names each open document's version, and writes nothing until the editor
applies it. Renaming a field or variant of a
[`@serial`](../../stdlib/serial-derive.md) type also changes the name it is
encoded under, so data written before the rename no longer decodes. The server
allows the rename, since no data may exist yet, and marks it with a change
annotation that needs confirmation, as it marks a change to a package's API.
A member with a [wire name](../../stdlib/serial-derive.md#wire-names) keeps its
encoded name, so renaming it is not marked.

The rename is checked twice. Before it, the package (or, for a local or an
alias, the file) must check without errors, so that every use is resolved.
After it, the edited files are checked again, and each renamed name and each
other name in them must still refer to the declaration it did before. The
server refuses the rename, with the reason as the request's error, when:

- the new name is not an identifier, is a reserved word, or is the old name;
- another name already written as the new name would conflict, or the
  rename would change what any name refers to (a local capturing a
  function, for instance). When the edited code would not check and the
  new name already appears there, the reason says so and gives the
  compile error;
- the package or the file does not check before the rename, or would not
  check after it;
- an [`if const`](../../language/02-source/target-conditionals.md) block
  mentions the old name, or the new name in a file where it would be
  written bare (the declaring module, a module importing it with `from`
  without an alias, or the file of a local or alias), since the arms a
  build does not select are not checked;
- one name in an uninstantiated generic body refers to different
  declarations in different instantiations;
- the declaration is in `std` or a dependency outside the workspace, or is
  written in a file no target or test of its package reads;
- the class or enum already has a member with the new name;
- the method is a protocol requirement, since implementations satisfy it by
  name and are not edited; or the old or new method name is a `__name__` hook
  or `display` or `debug`, which the language calls implicitly;
- an uninstantiated generic body writes the member's name on a value whose
  type it does not state, so the rename cannot tell whether it is a use;
- the name is `self`, or the callable already has a parameter with the new
  name;
- an uninstantiated generic body passes the parameter's name as a named
  argument to a callee it cannot resolve, so the rename cannot tell whether
  it is a use;
- the name is a module, or a module alias (`s` in `import shapes as s`, at
  the import or a use);
- the file changed since its last check; ask for the rename again.

Renaming a `pub` declaration of a package with a library, a public member
of one (a `pub` field or method, or any variant, of a `pub` type), or a
parameter of a public callable, changes that package's API, since packages outside the workspace that import it must
change. The server marks such an edit with a change annotation that needs
confirmation; an editor without change-annotation support shows the same
text as a warning.

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
  names and locals, and members only by the types written in it; see
  [find references](#finding-references).
- Builtin functions, methods of built-in types such as `list` and `str`, and
  `Ok`, `Err`, `Some`, and `Nothing` have no declaration to show.
- Locals offered by completion are those bound in the enclosing function
  above the cursor, by line.
- An error in a module other than the open file is shown on the file's first
  line, with the other file's position in the message.
- Each check reads the whole program again; there is no incremental reuse
  between checks. A program importing `std.http.server`, `std.json`, and five
  other large `std` modules checks in about 0.2 s with a release build.
- Document symbols and signature help are not implemented. Rename covers
  module-level declarations, members, parameters, import aliases, and
  locals; see [renaming](#renaming). Moving a declaration checks each
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
can use `plenty lsp` for files with the `.plenty`, `.p`, `.plentyi`, or `.pi` extension. No VS Code
extension is provided; a generic LSP client extension configured with the
command `plenty lsp` works.
