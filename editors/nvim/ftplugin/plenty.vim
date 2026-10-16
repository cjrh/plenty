if exists('b:did_ftplugin')
  finish
endif
let b:did_ftplugin = 1

setlocal expandtab shiftwidth=4 softtabstop=4
setlocal commentstring=#\ %s comments=:#
let b:undo_ftplugin = 'setlocal expandtab< shiftwidth< softtabstop< commentstring< comments<'
