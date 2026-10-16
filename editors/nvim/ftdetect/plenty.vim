augroup plenty_filetype
  autocmd!
  autocmd BufRead,BufNewFile *.plenty if &l:filetype ==# '' | setfiletype plenty | endif
augroup END
