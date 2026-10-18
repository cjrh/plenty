-- Language server for Plenty (Neovim 0.11+). `vim.lsp.enable('plenty')` finds
-- this file on 'runtimepath'. When `plenty` is not on PATH, set its absolute
-- path with `vim.lsp.config('plenty', { cmd = { '/path/to/plenty', 'lsp' } })`.
return {
  cmd = { 'plenty', 'lsp' },
  filetypes = { 'plenty' },
  root_markers = { 'plenty.toml', '.git' },
}
