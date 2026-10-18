-- Run from the repository root with a built compiler; no user configuration
-- or plugins are loaded:
--   PLENTY_BIN=target/debug/plenty nvim --headless -u NONE -i NONE -n -l editors/nvim/tests/lsp.lua
-- Drives the shipped configuration through Neovim's built-in LSP client:
-- diagnostics, hover (K), go to definition (gd) into a `std` source, and
-- completion.
local root = vim.fn.getcwd()
vim.opt.runtimepath:prepend(root .. '/editors/nvim')
vim.cmd('filetype plugin on')
local bin = vim.env.PLENTY_BIN or 'plenty'
if not bin:match('^/') and bin:find('/') then
  bin = root .. '/' .. bin
end
vim.lsp.config('plenty', { cmd = { bin, 'lsp' } })
vim.lsp.enable('plenty')

local dir = vim.fn.tempname()
vim.fn.mkdir(dir, 'p')
-- Std copies opened by go to definition go below the cache directory.
vim.env.PLENTY_CACHE_DIR = dir .. '/cache'
local file = dir .. '/main.plenty'
vim.fn.writefile({
  'from std.math import gcd',
  '',
  'class Point:',
  '    pub x: i64',
  '',
  '    def double(self) -> i64:',
  '        "Twice x."',
  '        self.x * 2',
  '',
  'def broken() -> i64:',
  '    "x"',
  '',
  'def main() -> ():',
  '    p = Point(3)',
  '    n = p.double()',
  '    g = gcd(6u64, 4u64)',
}, file)
vim.cmd('edit ' .. file)
assert(vim.bo.filetype == 'plenty', 'filetype')

local function wait(what, check)
  assert(vim.wait(20000, check, 50), 'timed out waiting for ' .. what)
end
wait('the client', function()
  return #vim.lsp.get_clients({ bufnr = 0, name = 'plenty' }) == 1
end)
wait('diagnostics', function()
  return #vim.diagnostic.get(0) > 0
end)
local diagnostics = vim.diagnostic.get(0)
assert(#diagnostics == 1, vim.inspect(diagnostics))
assert(diagnostics[1].lnum == 9, vim.inspect(diagnostics))
assert(diagnostics[1].message:find('expected i64, got %(%)'), diagnostics[1].message)

local function request(method, line, column)
  local params = {
    textDocument = vim.lsp.util.make_text_document_params(0),
    position = { line = line, character = column },
  }
  local results = vim.lsp.buf_request_sync(0, method, params, 10000)
  local _, response = next(results or {})
  assert(response and not response.err, method .. ': ' .. vim.inspect(response))
  return response.result
end

-- Hover on the method call in `p.double()`.
local hover = request('textDocument/hover', 14, 10)
assert(hover.contents.value:find('def double%(self%) %-> i64'), vim.inspect(hover))
assert(hover.contents.value:find('Twice x.'), vim.inspect(hover))

-- K opens the same hover in a floating window.
vim.api.nvim_win_set_cursor(0, { 15, 10 })
vim.cmd('normal K')
wait('the hover window', function()
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    if vim.api.nvim_win_get_config(win).relative ~= '' then
      return true
    end
  end
  return false
end)
vim.cmd('fclose!')

local items = request('textDocument/completion', 14, 10)
local labels = {}
for _, item in ipairs(items.items or items) do
  labels[item.label] = true
end
assert(labels.double and labels.x, vim.inspect(labels))

-- gd mapped as documented: jump into the embedded std module's copy.
vim.keymap.set('n', 'gd', vim.lsp.buf.definition, { buffer = 0 })
vim.api.nvim_win_set_cursor(0, { 16, 8 })
vim.cmd('normal gd')
wait('the std source', function()
  return vim.api.nvim_buf_get_name(0):find('std/math%.plenty$') ~= nil
end)
local line = vim.api.nvim_get_current_line()
assert(line:find('def gcd'), line)
print('ok')
vim.cmd('qall!')
