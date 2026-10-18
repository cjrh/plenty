# Neovim support

The repository includes a runtime plugin in `editors/nvim`. It detects
`*.plenty` files, highlights source syntax using the active colorscheme, sets
four-space soft tabs, and supplies `#` comment formatting.

Add this to `init.lua`, replacing the path with your Plenty checkout:

```lua
vim.opt.runtimepath:prepend('/absolute/path/to/plenty/editors/nvim')
vim.cmd('filetype plugin on')
vim.cmd('syntax enable')
```

Restart Neovim and open `examples/classes.plenty`. Check `:set filetype?` if
highlighting is missing; it should report `plenty`. The plugin directory's
README also describes native package installation and the headless test command.
Extension detection takes precedence over generic content detection, including
for source beginning with comments. An already-open buffer can be corrected with
`:setlocal filetype=plenty` after the runtime path is available.

Highlighting includes keywords, primitive and collection types, function
declarations, numbers and suffixes, strings, byte literals and escapes,
operators, and comments.
Single- and double-quoted strings stay on one line; triple-quoted strings span
lines. See [source syntax](../../language/02-source/index.md).

Highlighting uses regular expressions and does not perform semantic checks.
Reserved words can be colored even when their construct is not implemented.
Completion, formatting, automatic block indentation, LSP, and Tree-sitter are
not provided. Run `plenty check` in a package or `plentyc --check FILE` for
compiler diagnostics; see [compiler commands](commands.md).
