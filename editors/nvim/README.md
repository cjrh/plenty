# Plenty for Neovim

This runtime plugin detects `*.plenty` files and provides syntax highlighting,
four-space soft tabs, and `#` comment support. It needs no external dependencies.

Add the runtime directory to `init.lua`, before enabling filetypes and syntax
(replace the path with your checkout):

```lua
vim.opt.runtimepath:prepend('/absolute/path/to/plenty/editors/nvim')
vim.cmd('filetype plugin on')
vim.cmd('syntax enable')
```

Alternatively, symlink this directory into Neovim's native package directory:

```sh
mkdir -p ~/.config/nvim/pack/local/start
ln -s /absolute/path/to/plenty/editors/nvim ~/.config/nvim/pack/local/start/plenty
```

Use one installation method. Restart Neovim and open `examples/classes.plenty`
or a new `*.plenty` file. `:set filetype?` should report `plenty` and
`:echo b:current_syntax` should report `plenty`. Highlight groups use the active
colorscheme's standard colors. Users can override the `plenty*` highlight groups.
For other indentation preferences, use an `after/ftplugin/plenty.vim` file.

Highlights cover reserved words, primitive and collection types, function
declarations, booleans, `None`, decimal numbers with type suffixes, operators,
comments, escapes, and single/double/triple-quoted strings. The patterns follow
the public frontend in `src/frontend.rs`, not the internal stack-language lexer.
Reserved words are colored even when their syntax is not implemented yet.

This is regex highlighting: it does not resolve names, distinguish user-defined
types, diagnose invalid programs, or provide completion, formatting, automatic
block indentation, LSP, or Tree-sitter support. Triple-quoted strings are synced
from the start of the buffer for correctness; very large files may be slower.

Run the regression checks from the repository root:

```sh
nvim --headless -u NONE -i NONE -n -l editors/nvim/tests/highlighting.lua
```

The tests use a separate Neovim process and do not load or modify user settings.
Validated with Neovim 0.12.6.
