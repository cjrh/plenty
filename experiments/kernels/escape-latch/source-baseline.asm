==== unused  symbol=__plenty_fn_unused  kind=function  source=kernels.plenty:2
     0:  push    %rbp
     1:  mov     %rsp,%rbp
     4:  xor     %r8,%r8
     7:  cmp     %r8,%rdi
     a:  cmovge  %rdi,%r8
    # line 4
     e:  test    %r8,%r8
    11:  jle     0x0000000000000034
    17:  mulsd   0x40,%xmm0
    1f:  addsd   0x50,%xmm0
    27:  sub     $1,%r8
    2b:  test    %r8,%r8
    2e:  jg      0x0000000000000017
    # line 6
    34:  mov     %rbp,%rsp
    37:  pop     %rbp
    38:  ret
    39:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x9b,0xf2,0xd7,0x1a,0x00,0x00,0xf0,0x3f,0x00
    49:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xe0,0x3f,0x00
    59:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00

==== escape  symbol=__plenty_fn_escape  kind=function  source=kernels.plenty:8
     0:  push    %rbp
     1:  mov     %rsp,%rbp
    # line 9
     4:  xorpd   %xmm2,%xmm2
    # line 13
     8:  mov     $0x32,%eax
    # line 14
     d:  movupd  0x90,%xmm3
    15:  movdqa  %xmm2,%xmm4
    19:  movdqa  %xmm2,%xmm5
    1d:  movdqa  %xmm2,%xmm7
    21:  movdqa  %xmm2,%xmm6
    25:  addsd   %xmm5,%xmm6
    29:  ucomisd %xmm6,%xmm3
    2d:  jb      0x0000000000000079
    # line 16
    33:  subsd   %xmm5,%xmm2
    37:  addsd   %xmm0,%xmm2
    3b:  movdqa  %xmm2,%xmm6
    # line 17
    3f:  mulsd   %xmm2,%xmm6
    # line 15
    43:  addsd   %xmm4,%xmm4
    47:  mulsd   %xmm7,%xmm4
    4b:  addsd   %xmm1,%xmm4
    4f:  movdqa  %xmm4,%xmm5
    # line 18
    53:  mulsd   %xmm4,%xmm5
    57:  sub     $1,%rax
    # line 13
    5b:  test    %rax,%rax
    5e:  jle     0x0000000000000075
    64:  movdqa  %xmm4,%xmm7
    68:  movdqa  %xmm2,%xmm4
    6c:  movdqa  %xmm6,%xmm2
    70:  jmp     0x0000000000000021
    75:  movdqa  %xmm6,%xmm2
    # line 21
    79:  addsd   %xmm5,%xmm2
    7d:  ucomisd %xmm2,%xmm3
    81:  setae   %al
    84:  mov     %rbp,%rsp
    87:  pop     %rbp
    88:  ret
    89:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x10,0x40,0x00
    99:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00

==== main  symbol=__plenty_fn_main  kind=function  source=kernels.plenty:23
     0:  push    %rbp
     1:  mov     %rsp,%rbp
     4:  sub     $0x80,%rsp
    # line 24
     b:  movabs  $0x3FF0000000000000,%rax
    15:  movq    %rax,%xmm6
    1a:  mov     $0x14,%eax
    1f:  mulsd   0x170,%xmm6
    27:  addsd   0x180,%xmm6
    2f:  sub     $1,%rax
    # line 4
    33:  test    %rax,%rax
    36:  jg      0x000000000000001F
    # line 9
    3c:  xorpd   %xmm0,%xmm0
    # line 13
    40:  mov     $0x32,%eax
    # line 14
    45:  movupd  0x190,%xmm1
    4d:  movdqa  %xmm0,%xmm2
    51:  movdqa  %xmm0,%xmm3
    55:  movdqa  %xmm0,%xmm5
    59:  movdqa  %xmm0,%xmm4
    5d:  addsd   %xmm2,%xmm4
    61:  ucomisd %xmm4,%xmm1
    65:  jb      0x00000000000000B9
    # line 16
    6b:  subsd   %xmm2,%xmm0
    6f:  addsd   0x1A0,%xmm0
    77:  movdqa  %xmm0,%xmm4
    # line 17
    7b:  mulsd   %xmm0,%xmm4
    # line 15
    7f:  addsd   %xmm3,%xmm3
    83:  mulsd   %xmm5,%xmm3
    87:  addsd   0x1B0,%xmm3
    8f:  movdqa  %xmm3,%xmm2
    # line 18
    93:  mulsd   %xmm3,%xmm2
    97:  sub     $1,%rax
    # line 13
    9b:  test    %rax,%rax
    9e:  jle     0x00000000000000B5
    a4:  movdqa  %xmm3,%xmm5
    a8:  movdqa  %xmm0,%xmm3
    ac:  movdqa  %xmm4,%xmm0
    b0:  jmp     0x0000000000000059
    b5:  movdqa  %xmm4,%xmm0
    # line 24
    b9:  movq    %xmm6,%rdi
    be:  xor     %r9,%r9
    c1:  lea     (%rsp),%r8
    c5:  mov     %rdi,(%rsp)
    c9:  mov     %r9,8(%rsp)
    # line 21
    ce:  addsd   %xmm2,%xmm0
    d2:  ucomisd %xmm0,%xmm1
    d6:  setae   %r10b
    da:  movzbq  %r10b,%r9
    de:  xor     %r11,%r11
    # line 24
    e1:  mov     %r9,0x10(%rsp)
    e6:  mov     %r11,0x18(%rsp)
    eb:  lea     0x20(%rsp),%rdi
    f0:  mov     %r8,0x20(%rsp)
    f5:  xor     %rdx,%rdx
    f8:  movq    $0,0x28(%rsp)
   101:  movq    $0,0x30(%rsp)
   10a:  movq    $0,0x38(%rsp)
   113:  movq    $0,0x40(%rsp)
   11c:  movq    $0,0x48(%rsp)
   125:  lea     0x12C,%rsi  # .Ldata1 (metadata tuple[f64, bool])
   12c:  call    0x0000000000000131  # plenty_op_166
   131:  mov     0x50(%rsp),%rax
   136:  mov     0x58(%rsp),%rcx
   13b:  test    $1,%rcx
   142:  je      0x0000000000000158
   148:  or      $1,%rcx
   14c:  add     $0x80,%rsp
   153:  mov     %rbp,%rsp
   156:  pop     %rbp
   157:  ret
   158:  xor     %rax,%rax
   15b:  xor     %rcx,%rcx
    # line 25
   15e:  add     $0x80,%rsp
   165:  mov     %rbp,%rsp
   168:  pop     %rbp
   169:  ret
   16a:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x9b,0xf2,0xd7,0x1a,0x00,0x00,0xf0,0x3f,0x00,0x00
   17a:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0xe0,0x3f,0x00,0x00
   18a:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x10,0x40,0x00,0x00
   19a:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x9a,0x99,0x99,0x99,0x99,0x99,0xc9,0xbf,0x00,0x00
   1aa:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x9a,0x99,0x99,0x99,0x99,0x99,0xd9,0x3f,0x00,0x00
   1ba:  .byte   0x00,0x00,0x00,0x00,0x00,0x00

==== plenty_main  symbol=plenty_main  kind=entry  source=-
     0:  push    %rbp
     1:  mov     %rsp,%rbp
     4:  sub     $0x60,%rsp
     8:  lea     0xF,%rdi  # .Ldata0
     f:  call    0x0000000000000014  # plenty_traps_register
    14:  call    0x0000000000000019  # __plenty_fn_main
    19:  test    $1,%rcx
    20:  je      0x0000000000000078
    26:  shr     $1,%rcx
    29:  lea     (%rsp),%rdi
    2d:  mov     %rax,(%rsp)
    31:  mov     %rcx,8(%rsp)
    36:  xor     %rdx,%rdx
    39:  movq    $0,0x10(%rsp)
    42:  movq    $0,0x18(%rsp)
    4b:  movq    $0,0x20(%rsp)
    54:  movq    $0,0x28(%rsp)
    5d:  lea     0x64,%rsi  # .Ldatac (metadata IoError)
    64:  call    0x0000000000000069  # plenty_op_122
    69:  mov     0x30(%rsp),%rax
    6e:  mov     0x38(%rsp),%rcx
    73:  jmp     0x000000000000007A
    78:  xor     %eax,%eax
    7a:  add     $0x60,%rsp
    7e:  mov     %rbp,%rsp
    81:  pop     %rbp
    82:  ret
