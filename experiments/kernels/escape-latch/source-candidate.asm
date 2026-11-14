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
    1d:  movdqa  %xmm2,%xmm6
    21:  movdqa  %xmm2,%xmm7
    25:  addsd   %xmm5,%xmm7
    29:  ucomisd %xmm7,%xmm3
    2d:  jb      0x0000000000000071
    # line 16
    33:  subsd   %xmm5,%xmm2
    37:  addsd   %xmm0,%xmm2
    3b:  movdqa  %xmm2,%xmm7
    # line 17
    3f:  mulsd   %xmm2,%xmm7
    # line 15
    43:  addsd   %xmm4,%xmm4
    47:  mulsd   %xmm6,%xmm4
    4b:  addsd   %xmm1,%xmm4
    4f:  movdqa  %xmm4,%xmm5
    # line 18
    53:  mulsd   %xmm4,%xmm5
    # line 13
    57:  sub     $1,%rax
    5b:  movdqa  %xmm4,%xmm6
    5f:  movdqa  %xmm2,%xmm4
    63:  movdqa  %xmm7,%xmm2
    67:  jne     0x0000000000000021
    6d:  movdqa  %xmm7,%xmm2
    # line 21
    71:  addsd   %xmm5,%xmm2
    75:  ucomisd %xmm2,%xmm3
    79:  setae   %al
    7c:  mov     %rbp,%rsp
    7f:  pop     %rbp
    80:  ret
    81:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00
    91:  .byte   0x00,0x00,0x00,0x00,0x00,0x10,0x40,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00

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
    55:  movdqa  %xmm0,%xmm4
    59:  movdqa  %xmm0,%xmm5
    5d:  addsd   %xmm2,%xmm5
    61:  ucomisd %xmm5,%xmm1
    65:  jb      0x00000000000000B1
    # line 16
    6b:  subsd   %xmm2,%xmm0
    6f:  addsd   0x1A0,%xmm0
    77:  movdqa  %xmm0,%xmm5
    # line 17
    7b:  mulsd   %xmm0,%xmm5
    # line 15
    7f:  addsd   %xmm3,%xmm3
    83:  mulsd   %xmm4,%xmm3
    87:  addsd   0x1B0,%xmm3
    8f:  movdqa  %xmm3,%xmm2
    # line 18
    93:  mulsd   %xmm3,%xmm2
    # line 13
    97:  sub     $1,%rax
    9b:  movdqa  %xmm3,%xmm4
    9f:  movdqa  %xmm0,%xmm3
    a3:  movdqa  %xmm5,%xmm0
    a7:  jne     0x0000000000000059
    ad:  movdqa  %xmm5,%xmm0
    # line 24
    b1:  movq    %xmm6,%rdi
    b6:  xor     %r8,%r8
    b9:  lea     (%rsp),%rsi
    bd:  mov     %rdi,(%rsp)
    c1:  mov     %r8,8(%rsp)
    # line 21
    c6:  addsd   %xmm2,%xmm0
    ca:  ucomisd %xmm0,%xmm1
    ce:  setae   %r9b
    d2:  movzbq  %r9b,%rdi
    d6:  xor     %r10,%r10
    # line 24
    d9:  mov     %rdi,0x10(%rsp)
    de:  mov     %r10,0x18(%rsp)
    e3:  lea     0x20(%rsp),%rdi
    e8:  mov     %rsi,0x20(%rsp)
    ed:  xor     %rdx,%rdx
    f0:  movq    $0,0x28(%rsp)
    f9:  movq    $0,0x30(%rsp)
   102:  movq    $0,0x38(%rsp)
   10b:  movq    $0,0x40(%rsp)
   114:  movq    $0,0x48(%rsp)
   11d:  lea     0x124,%rsi  # .Ldata1 (metadata tuple[f64, bool])
   124:  call    0x0000000000000129  # plenty_op_166
   129:  mov     0x50(%rsp),%rax
   12e:  mov     0x58(%rsp),%rcx
   133:  test    $1,%rcx
   13a:  je      0x0000000000000150
   140:  or      $1,%rcx
   144:  add     $0x80,%rsp
   14b:  mov     %rbp,%rsp
   14e:  pop     %rbp
   14f:  ret
   150:  xor     %rax,%rax
   153:  xor     %rcx,%rcx
    # line 25
   156:  add     $0x80,%rsp
   15d:  mov     %rbp,%rsp
   160:  pop     %rbp
   161:  ret
   162:  .byte   0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x9b,0xf2
   172:  .byte   0xd7,0x1a,0x00,0x00,0xf0,0x3f,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00
   182:  .byte   0x00,0x00,0x00,0x00,0xe0,0x3f,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00
   192:  .byte   0x00,0x00,0x00,0x00,0x10,0x40,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x9a,0x99
   1a2:  .byte   0x99,0x99,0x99,0x99,0xc9,0xbf,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x9a,0x99
   1b2:  .byte   0x99,0x99,0x99,0x99,0xd9,0x3f,0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00

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
