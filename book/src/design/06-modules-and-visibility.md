# Modules and visibility

`import package.module`, `import package.module as alias`, and
`from package.module import Name as Alias` bind explicit names. Comma-separated
imports are supported. Imports exist only at module scope and do not execute
initializers. There are no globals, implicit transitive imports, relative paths,
wildcards, or re-exports. A module's imported names are private bindings.

One source root defaults to the entry file's canonical directory and can be set
with `--module-root DIR`. `a.b` resolves to `<root>/a/b.plenty` or the trusted
interface `<root>/a/b.plentyi`; having both is an ambiguity error. Directories are
namespaces, without `__init__` execution. File/directory name collisions are
errors. Canonical paths deduplicate imports and cannot escape the selected root.
Imported canonical filenames and directories must have identifier components
and a `.plenty` or `.plentyi` extension. Cycles report the actual file dependency chain. The
initial implementation caps graph depth at 128 and loaded modules at 4096.

`pub` exposes top-level functions, aliases, classes, and enums. Private names
are accessible only within their defining module; directory ancestry adds no
privileges. Class fields and methods require their own `pub`. Generated field
constructors are public only for public classes with all-public fields; an
explicit `__init__` has its own visibility. Public factories can construct
otherwise private constructors within their defining module. Automatic `__del__`
invocation is unaffected by privacy; direct lifecycle calls remain prohibited.

Public enums expose every variant. Resolved public signatures, public fields,
enum payloads, and public aliases cannot expose private nominal types, including
through transparent aliases and nested containers. Access checks cover field
reads, writes, borrows, method calls, and construction through aliases. Generated
structural printing and equality still include private fields; privacy is not
data secrecy. `pub` has no C ABI or binary-export meaning.

The compilation session loads each canonical file once, resolves explicit
bindings and lexical shadows, and assigns imported declarations qualified names
before type resolution. These names preserve nominal identity across import
aliases and diamond dependencies. Entry-module declarations retain their local
names; dependency declarations use canonical dotted module prefixes. All reachable
modules are checked and emitted into one object. Source paths accompany frontend
and independent IR/ownership diagnostics. Separate compilation and persistent
module caching are not implemented yet.
