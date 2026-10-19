-- Run from the repository root with a built compiler; no user configuration
-- or plugins are loaded:
--   PLENTY_BIN=target/debug/plenty nvim --headless -u NONE -i NONE -n -l editors/nvim/tests/lsp.lua
-- Drives the shipped configuration through Neovim's built-in LSP client:
-- diagnostics, hover (K), go to definition (gd) into a `std` source,
-- completion, and re-checks after an edit (didChange) and a write (didSave).
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
local buf = vim.api.nvim_get_current_buf()

local function wait(what, check)
  assert(vim.wait(20000, check, 50), 'timed out waiting for ' .. what)
end
wait('the client', function()
  return #vim.lsp.get_clients({ bufnr = 0, name = 'plenty' }) == 1
end)
-- Record the notifications the client sends, to show that edits and writes
-- reach the server through didChange and didSave.
local client = vim.lsp.get_clients({ bufnr = 0, name = 'plenty' })[1]
local sent = {}
local notify = client.notify
client.notify = function(self, method, ...)
  sent[method] = true
  return notify(self, method, ...)
end
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

-- didChange: completing `broken` clears its diagnostic after the server's
-- debounced re-check of the unsaved text.
vim.api.nvim_set_current_buf(buf)
vim.api.nvim_buf_set_lines(buf, 11, 11, false, { '    0' })
wait('diagnostics after the edit', function()
  return #vim.diagnostic.get(buf) == 0
end)
assert(sent['textDocument/didChange'], vim.inspect(sent))

-- didSave: a new error written to disk is reported at its line.
vim.api.nvim_buf_set_lines(buf, -1, -1, false, { '    k = p.double(1)' })
vim.cmd('write')
assert(sent['textDocument/didSave'], vim.inspect(sent))
wait('diagnostics after the write', function()
  return #vim.diagnostic.get(buf) > 0
end)
diagnostics = vim.diagnostic.get(buf)
assert(#diagnostics == 1, vim.inspect(diagnostics))
assert(diagnostics[1].lnum == 17, vim.inspect(diagnostics))
assert(diagnostics[1].message:find('expects 0 arguments'), diagnostics[1].message)
print('ok')
vim.cmd('qall!')
