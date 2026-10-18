# Neovim support

The repository includes a runtime plugin in `editors/nvim`. It detects
`*.plenty` files, highlights source syntax using the active colorscheme, sets
four-space soft tabs, supplies `#` comment formatting, and configures the
[language server](language-server.md) for Neovim's built-in LSP client.

Add this to `init.lua`, replacing the path with your Plenty checkout:

```lua
vim.opt.runtimepath:prepend('/absolute/path/to/plenty/editors/nvim')
vim.cmd('filetype plugin on')
vim.cmd('syntax enable')
```

Restart Neovim and open `examples/classes.plenty`. Check `:set filetype?` if
highlighting is missing; it should report `plenty`. The plugin directory's
README also describes native package installation and the headless test commands.
Extension detection takes precedence over generic content detection, including
for source beginning with comments. An already-open buffer can be corrected with
`:setlocal filetype=plenty` after the runtime path is available.

Highlighting includes keywords, primitive and collection types, function
declarations, numbers and suffixes, strings, byte literals and escapes,
operators, and comments.
Single- and double-quoted strings stay on one line; triple-quoted strings span
lines. See [source syntax](../../language/02-source/index.md). Highlighting
uses regular expressions and does not perform semantic checks. Reserved words
can be colored even when their construct is not implemented.

## Language server

With Neovim 0.11 or later, `editors/nvim/lsp/plenty.lua` configures the
server. Enable it after the runtime path above:

```lua
vim.lsp.enable('plenty')
-- If `plenty` is not on PATH:
-- vim.lsp.config('plenty', { cmd = { '/path/to/plenty', 'lsp' } })

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

`K` shows the hover. `gd` goes to the definition, into a read-only copy of a
`std` module when the definition is there; `CTRL-]` does the same through
Neovim's default LSP tag function. Errors appear as diagnostics; `]d` and `[d`
move between them. Completion triggers after `.` and with `CTRL-X CTRL-O`.
With nvim-cmp instead of the built-in completion, leave out the
`vim.lsp.completion.enable` line and add the `nvim_lsp` source.

Neovim 0.10 has no `vim.lsp.config`; start the server from a `FileType`
autocommand instead:

```lua
vim.api.nvim_create_autocmd('FileType', {
  pattern = 'plenty',
  callback = function()
    vim.lsp.start({
      name = 'plenty',
      cmd = { 'plenty', 'lsp' },
      root_dir = vim.fs.root(0, { 'plenty.toml', '.git' }),
    })
  end,
})
```

Formatting, automatic block indentation, and Tree-sitter are not provided.
