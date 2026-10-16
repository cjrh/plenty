-- Resolve the extension before Neovim's content-based conf fallback runs.
vim.filetype.add({
  extension = {
    plenty = function(_, bufnr)
      -- Replaying detection must not replace a user's explicit buffer choice.
      local current = vim.bo[bufnr].filetype
      return current ~= '' and current or 'plenty'
    end,
  },
})
