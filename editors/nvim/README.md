# Plenty for Neovim

This runtime plugin detects `*.plenty` files and provides syntax highlighting,
four-space soft tabs, `#` comment support, and a configuration for the Plenty
language server (`plenty lsp`) in `lsp/plenty.lua`.

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

The plugin registers the extension with Neovim's native filetype detector so
comment-leading source does not fall through to generic `conf` detection. If
you added the runtime path after opening a file, restart Neovim to load detection;
for an already-open buffer with the runtime path available, run
`:setlocal filetype=plenty` to load its syntax and editing settings immediately.

Highlights cover reserved words, primitive and collection types, function
declarations, booleans, `None`, decimal numbers with type suffixes, operators,
comments, escapes, and single/double/triple-quoted strings. The patterns follow
the public frontend in `src/frontend.rs`, not the internal stack-language lexer.
Reserved words are colored even when their syntax is not implemented yet.

Highlighting uses regular expressions; triple-quoted strings are synced from
the start of the buffer for correctness, so very large files may be slower.

## Language server

The server gives diagnostics, hover, go to definition (including into `std`),
and completion. With Neovim 0.11 or later and `plenty` on `PATH`, add after the
runtime path:

```lua
vim.lsp.enable('plenty')
vim.api.nvim_create_autocmd('LspAttach', {
  callback = function(event)
    local client = vim.lsp.get_client_by_id(event.data.client_id)
    if client.name ~= 'plenty' then
      return
    end
    vim.keymap.set('n', 'gd', vim.lsp.buf.definition, { buffer = event.buf })
    vim.lsp.completion.enable(true, client.id, event.buf, { autotrigger = true })
  end,
})
```

Otherwise set the command first:
`vim.lsp.config('plenty', { cmd = { '/path/to/plenty', 'lsp' } })`. `K` shows
a declaration and its docstring. The book's Neovim page covers nvim-cmp and
Neovim 0.10.

Run the regression checks from the repository root, the second with a built
compiler:

```sh
nvim --headless -u NONE -i NONE -n -l editors/nvim/tests/highlighting.lua
PLENTY_BIN=target/debug/plenty nvim --headless -u NONE -i NONE -n -l editors/nvim/tests/lsp.lua
```

The tests use a separate Neovim process and do not load or modify user settings.
`cargo test` runs the second one when `nvim` is installed. Validated with
Neovim 0.12.6.
