# Editing TOML documents

`std.toml.Document` retains validated TOML 1.0 source text and a parser index.
It takes inspiration from [toml_edit](https://docs.rs/toml_edit/), but is
implemented in checked Plenty with fallible allocation, alongside the
[existing parser and writer](toml.md).

```text
document(source: &Bytes, bounds: &Limits) -> Result[Document, TomlError]
document_text(text: &str, bounds: &Limits) -> Result[Document, TomlError]
Document.text() -> Result[str, TomlError]
Document.get(path: &list[str]) -> Result[Option[Toml], TomlError]
Document.set(path: &list[str], value: &Toml) -> Result[(), TomlError]
Document.insert(path: &list[str], value: &Toml) -> Result[(), TomlError]
Document.remove(path: &list[str]) -> Result[bool, TomlError]
```

Mutation methods take `&mut Document`. Paths contain decoded key components:
`["package", "version"]` addresses `package.version`, whereas `["a.b"]`
addresses the single quoted key `"a.b"`. `get` returns an owned value, or
`Nothing` for a missing key. An empty path reads the complete root table;
mutating the root is rejected with `KeyConflict`.

`set` replaces an existing value or inserts a missing one. `insert` requires
absence (`DuplicateKey` otherwise). For insertion of a scalar or array, its
parent table must already exist. Insert a `Toml.Table` to create a section,
including empty tables and implicit ancestors. Existing section tables can
only be replaced with tables; replacing a section with a scalar is rejected.
`remove` returns whether a key existed and removes a table's descendant
statements, even when its sections are separated in the source.

## Preservation contract

- Parsing followed by `text` returns the exact original source, including
  comments, whitespace, key spelling, line endings and absence of a final newline.
- Replacing an assignment changes only its value token. Its key, spaces around
  `=`, trailing comment and newline remain byte-for-byte unchanged. The new
  value uses the normal writer's spelling; replacing an array or inline table
  replaces its entire token, including comments inside that token.
- Insertion adds a canonically formatted entry at the end of its parent section,
  or appends a new section. New text uses LF line endings; original CRLF bytes
  remain untouched. A missing newline at an insertion boundary is added.
- Removing a key or section removes the complete associated statement lines,
  including their trailing comments. Standalone comments and blank lines remain
  exactly where they were relative to other surviving text. The editor does
  not guess which standalone comment belongs to a removed key.
- Replacing a section removes its old statements and appends a newly formatted
  section. Unrelated statements and standalone comments retain their bytes.

## Boundaries and costs

Arrays, including arrays of tables, are atomic values. `get` can read them and
`remove` can delete an entire array of tables, but a path through an array returns
`KeyConflict`; there is no implicit "last row" selection. Inline-table fields
can be read individually, but edits must replace the whole inline table. They
have no independent document statements to insert or remove. There is no
comment-editing, key-renaming, array-index path, or custom-formatting API.

Each mutation builds a candidate source and reparses it before committing.
All errors, including `TomlFault.Allocation(AllocError)`, leave the document
unchanged. Unsupported paths return `KeyConflict`; syntax conflicts discovered
by validation retain the parser's specific fault and candidate-source location.
This is an in-memory editing API, not an atomic file-writing API.

The document retains the source, arena and statement spans. Editing costs a
full source copy and parse; `get` copies the requested tree. This deliberately
favours a small, checked implementation for manifests over large-document
incremental editing. `Limits` are captured at construction: input and depth
limits apply to both original and candidate text, and output limits bound
candidate construction. No Rust dependency or infallible allocator wrapper is
introduced.

## Acknowledgements

- Design follows the Rust [`toml_edit`](https://docs.rs/toml_edit/) crate.
