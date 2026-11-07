-- Resolve extensions before Neovim's content-based conf/Pascal fallback runs.
local function plenty(_, bufnr)
  -- Replaying detection must not replace a user's explicit buffer choice.
  local current = vim.bo[bufnr].filetype
  return current ~= '' and current or 'plenty'
end
vim.filetype.add({ extension = { plenty = plenty, plentyi = plenty, p = plenty, pi = plenty } })
