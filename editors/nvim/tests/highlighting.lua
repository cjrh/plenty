-- Run from the repository root; no user configuration or plugins are loaded.
vim.opt.runtimepath:prepend(vim.fn.getcwd() .. '/editors/nvim')
vim.cmd('filetype plugin on')
vim.cmd('syntax enable')
vim.cmd('edit examples/classes.plenty')
assert(vim.bo.filetype == 'plenty', 'existing file detection')
vim.cmd('edit stdlib/std/bytes.plenty')
assert(vim.bo.filetype == 'plenty', 'comment-leading source must beat conf fallback')
assert(vim.bo.syntax == 'plenty' and vim.b.current_syntax == 'plenty', 'source syntax loaded')
vim.cmd('enew')
vim.cmd('file syntax-test.plenty')
vim.cmd('doautocmd BufNewFile')
assert(vim.bo.filetype == 'plenty', 'new file detection')
assert(vim.bo.expandtab and vim.bo.shiftwidth == 4 and vim.bo.softtabstop == 4)
assert(vim.bo.commentstring == '# %s' and vim.bo.comments == ':#')

local cases = {
  {'def example(x: i64) -> bool:', 'def', 'plentyKeyword'},
  {'def example(x: i64) -> bool:', 'example', 'plentyFunction'},
  {'def example(x: i64) -> bool:', 'i64', 'plentyType'},
  {'if True and not False:', 'True', 'plentyBoolean'},
  {'if True and not False:', 'and', 'plentyOperator'},
  {'for x in items:', 'for', 'plentyRepeat'},
  {'from std import math', 'import', 'plentyInclude'},
  {'value = None', 'None', 'plentyConstant'},
  {'value = 1_024u64', '1_024u64', 'plentyNumber'},
  {'value = 3.25e-2f32', '3.25e-2f32', 'plentyFloat'},
  {'value = .5', '.5', 'plentyFloat'},
  {'value = 1.', '1.', 'plentyFloat'},
  {'value = 1e+3', '1e+3', 'plentyFloat'},
  {'value = 32f64', '32f64', 'plentyFloat'},
  {'value = call()?', '?', 'plentyOperator'},
  {'items = [1, 2]', '[', 'plentyDelimiter'},
  {'items = [1, 2]', ']', 'plentyDelimiter'},
  {'items = {1: 2}', '{', 'plentyDelimiter'},
  {'items = {1: 2}', '}', 'plentyDelimiter'},
  {'# if True 123 "comment"', 'True', 'plentyComment'},
  {'# TODO fix this', 'TODO', 'plentyTodo'},
  {'value = "# if True 123"', 'True', 'plentyString'},
  {[[value = 'it\'s # fine']], '#', 'plentyString'},
  {[[value = "a\"#b"]], '#', 'plentyString'},
  {[[value = "line\n"]], [[\n]], 'plentyEscape'},
  {[[value = "e\u{301}"]], [[\u{301}]], 'plentyEscape'},
  {'value = 0xFFu8', '0xFFu8', 'plentyNumber'},
  {'value = 0x1E', '0x1E', 'plentyNumber'},
  {'value = 0x1f32', '0x1f32', 'plentyNumber'},
  {'value = 0o755', '0o755', 'plentyNumber'},
  {'value = 0b1010_0101', '0b1010_0101', 'plentyNumber'},
  {[[value = "\\" # trailing]], 'trailing', 'plentyComment'},
  {'@copy', '@copy', 'plentyAnnotation'},
  {'identifier123 = true', '123', ''},
  {'identifier123 = true', 'true', ''},
}
local lines = {}
for _, case in ipairs(cases) do table.insert(lines, case[1]) end
vim.list_extend(lines, {'text = """first', '# if True 123', 'last"""', 'return False',
  "text = '''first", '# other string', "last'''", 'return True',
  'broken = "unfinished', 'return True'})
vim.api.nvim_buf_set_lines(0, 0, -1, false, lines)
vim.cmd('syntax sync fromstart')
local function group(line, col)
  return vim.fn.synIDattr(vim.fn.synID(line, col, 1), 'name')
end
for line, case in ipairs(cases) do
  local col = assert(case[1]:find(case[2], 1, true))
  for offset = 0, #case[2] - 1 do
    assert(group(line, col + offset) == case[3],
      ('line %d col %d: expected %s, got %s'):format(line, col + offset, case[3], group(line, col + offset)))
  end
end
assert(group(#cases + 2, 1) == 'plentyString', 'multiline double quote')
assert(group(#cases + 4, 1) == 'plentyKeyword', 'triple quote ends')
assert(group(#cases + 6, 1) == 'plentyString', 'multiline single quote')
assert(group(#cases + 8, 1) == 'plentyKeyword', 'triple single quote ends')
assert(group(#cases + 10, 1) == 'plentyKeyword', 'unfinished single string cannot leak')
vim.cmd('setlocal filetype=text')
assert(vim.bo.shiftwidth == vim.go.shiftwidth, 'ftplugin options restored')
vim.cmd('doautocmd BufRead')
assert(vim.bo.filetype == 'text', 'detection preserves an explicit filetype')
print('Plenty Neovim highlighting and filetype tests passed')
vim.cmd('qa!')
