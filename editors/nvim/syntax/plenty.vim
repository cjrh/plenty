if exists('b:current_syntax')
  finish
endif

syntax case match
syntax keyword plentyKeyword extern export opaque try with type const return mut pass
syntax keyword plentyKeyword def nextgroup=plentyFunction skipwhite
syntax keyword plentyKeyword struct enum yield async await class lambda trait protocol pub
syntax keyword plentyConditional if elif else match case
syntax keyword plentyRepeat while for break continue
syntax keyword plentyInclude import from as
syntax keyword plentyOperator and or not in
syntax keyword plentyBoolean True False
syntax keyword plentyConstant None
syntax keyword plentyType i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 bool str bytes
syntax keyword plentyType list dict set tuple
syntax match plentyOperator /[-+*\/%=<>!&|^~?]/
syntax match plentyDelimiter /[()[\]{},.:]/
syntax match plentyFunction /[A-Za-z_][A-Za-z_0-9]*/ contained
" A declaration annotation, such as `@copy`, alone on its line.
syntax match plentyAnnotation /^\s*\zs@[A-Za-z_][A-Za-z_0-9]*/

" A leading sign is an operator; suffixes belong to the literal.
syntax match plentyNumber /\<\d[0-9_]*\%([iu]\%(8\|16\|32\|64\)\)\?\>/
syntax match plentyFloat /\<\d[0-9_]*\%([eE][+-]\?[0-9_]\+\)\%(f32\|f64\)\?\>/
syntax match plentyFloat /\<\d[0-9_]*\%(f32\|f64\)\>/
syntax match plentyFloat /\%([A-Za-z_0-9]\)\@<!\%(\d[0-9_]*\.[0-9_]*\|\.\d[0-9_]*\)\%([eE][+-]\?[0-9_]\+\)\?\%(f32\|f64\)\?/
" Hexadecimal, octal, and binary forms are always integers.
syntax match plentyNumber /\<0[xX][0-9A-Fa-f_]\+\%([iu]\%(8\|16\|32\|64\)\)\?\>/
syntax match plentyNumber /\<0[oO][0-7_]\+\%([iu]\%(8\|16\|32\|64\)\)\?\>/
syntax match plentyNumber /\<0[bB][01_]\+\%([iu]\%(8\|16\|32\|64\)\)\?\>/

syntax keyword plentyTodo TODO FIXME XXX NOTE contained
syntax match plentyComment /#.*/ contains=plentyTodo,@Spell
syntax match plentyEscape /\\\%([0nrt\\'\"]\|u{\x\{1,6}}\)/ contained
syntax region plentyString start=/"/ skip=/\\./ end=/"/ oneline contains=plentyEscape,@Spell
syntax region plentyString start=/'/ skip=/\\./ end=/'/ oneline contains=plentyEscape,@Spell
" Later definitions take precedence over the single-quote regions.
syntax region plentyString start=/"""/ skip=/\\./ end=/"""/ contains=plentyEscape,@Spell
syntax region plentyString start=/'''/ skip=/\\./ end=/'''/ contains=plentyEscape,@Spell
" f-strings: `{{` and `}}` are literal braces; a field is ordinary code,
" and its spec may hold nested fields.
syntax match plentyFormatBrace /{{\|}}/ contained
syntax region plentyFormatField matchgroup=plentySpecial start=/{\%({\)\@!/ end=/}/ contained contains=plentyFormatField,plentyKeyword,plentyBoolean,plentyType,plentyNumber,plentyFloat,plentyOperator,plentyDelimiter,plentyString
syntax region plentyFormatString start=/\<f"/ skip=/\\./ end=/"/ oneline contains=plentyEscape,plentyFormatBrace,plentyFormatField,@Spell
syntax region plentyFormatString start=/\<f'/ skip=/\\./ end=/'/ oneline contains=plentyEscape,plentyFormatBrace,plentyFormatField,@Spell
syntax region plentyFormatString start=/\<f"""/ skip=/\\./ end=/"""/ contains=plentyEscape,plentyFormatBrace,plentyFormatField,@Spell
syntax region plentyFormatString start=/\<f'''/ skip=/\\./ end=/'''/ contains=plentyEscape,plentyFormatBrace,plentyFormatField,@Spell
" Byte literals: ASCII text with `\xHH` escapes, not spell-checked.
syntax match plentyByteEscape /\\\%(x\x\x\|[0nrt\\'"]\)/ contained
syntax region plentyBytes start=/\<b"/ skip=/\\./ end=/"/ oneline contains=plentyByteEscape
syntax region plentyBytes start=/\<b'/ skip=/\\./ end=/'/ oneline contains=plentyByteEscape
syntax region plentyBytes start=/\<b"""/ skip=/\\./ end=/"""/ contains=plentyByteEscape
syntax region plentyBytes start=/\<b'''/ skip=/\\./ end=/'''/ contains=plentyByteEscape
syntax sync fromstart

highlight default link plentyKeyword Keyword
highlight default link plentyConditional Conditional
highlight default link plentyRepeat Repeat
highlight default link plentyInclude Include
highlight default link plentyOperator Operator
highlight default link plentyBoolean Boolean
highlight default link plentyConstant Constant
highlight default link plentyType Type
highlight default link plentyDelimiter Delimiter
highlight default link plentyFunction Function
highlight default link plentyAnnotation PreProc
highlight default link plentyNumber Number
highlight default link plentyFloat Float
highlight default link plentyTodo Todo
highlight default link plentyComment Comment
highlight default link plentyEscape SpecialChar
highlight default link plentyString String
highlight default link plentyFormatString String
highlight default link plentyFormatBrace SpecialChar
highlight default link plentySpecial Special
highlight default link plentyBytes String
highlight default link plentyByteEscape SpecialChar

let b:current_syntax = 'plenty'
