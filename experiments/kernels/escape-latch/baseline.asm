==== experiments/kernels/counted-latch/escape.clif %a
function %a(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    jump block2

block3:
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v10 = iconst.i64 1
    v2 = iconst.i64 0
    jump block4(v4, v4, v4, v4, v9)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50

block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v28 = fadd v26, v27
    v32 = fcmp le v28, v31
    brif v32, block7, block8

block7:
    jump block6

block8:
    jump block1(v26, v27)

block6:
    jump block5

block5:
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    v109 = iconst.i64 1
    v110 = isub.i64 v45, v109  ; v109 = 1
    v111 = iconst.i64 0
    v112 = icmp sgt v110, v111  ; v111 = 0
    brif v112, block4(v43, v44, v42, v39, v110), block1(v43, v44)

block2:
    jump block3

block1(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  xorpd %xmm2, %xmm2
     8:  movl $0x32, %eax
     d:  movupd 0x7b(%rip), %xmm3
    15:  movdqa %xmm2, %xmm4
    19:  movdqa %xmm2, %xmm5
    1d:  movdqa %xmm2, %xmm7
    21:  movdqa %xmm2, %xmm6
    25:  addsd %xmm5, %xmm6
    29:  ucomisd %xmm6, %xmm3
    2d:  jb 0x79
    33:  subsd %xmm5, %xmm2
    37:  addsd %xmm0, %xmm2
    3b:  movdqa %xmm2, %xmm6
    3f:  mulsd %xmm2, %xmm6
    43:  addsd %xmm4, %xmm4
    47:  mulsd %xmm7, %xmm4
    4b:  addsd %xmm1, %xmm4
    4f:  movdqa %xmm4, %xmm5
    53:  mulsd %xmm4, %xmm5
    57:  subq $1, %rax
    5b:  testq %rax, %rax
    5e:  jle 0x75
    64:  movdqa %xmm4, %xmm7
    68:  movdqa %xmm2, %xmm4
    6c:  movdqa %xmm6, %xmm2
    70:  jmp 0x21
    75:  movdqa %xmm6, %xmm2
    79:  addsd %xmm5, %xmm2
    7d:  ucomisd %xmm2, %xmm3
    81:  setae %al
    84:  movq %rbp, %rsp
    87:  popq %rbp
    88:  retq
    89:  addb %al, (%rax)
    8b:  addb %al, (%rax)
    8d:  addb %al, (%rax)
    8f:  addb %al, (%rax)
    91:  addb %al, (%rax)
    93:  addb %al, (%rax)
    95:  addb %dl, (%rax)
    97:  addb %al, (%rax)
    9a:  addb %al, (%rax)
    9c:  addb %al, (%rax)
    9e:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %b
function %b(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    jump block2

block3:
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v10 = iconst.i64 1
    v2 = iconst.i64 0
    jump block4(v4, v4, v4, v4, v9)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50

block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v28 = fadd v26, v27
    v32 = fcmp le v28, v31
    brif v32, block7, block8

block7:
    jump block6

block8:
    jump block1(v26, v27)

block6:
    jump block5

block5:
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    v109 = iconst.i64 1
    v110 = isub.i64 v45, v109  ; v109 = 1
    v111 = iconst.i64 0
    v112 = icmp sle v110, v111  ; v111 = 0
    brif v112, block1(v43, v44), block4(v43, v44, v42, v39, v110)

block2:
    jump block3

block1(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  xorpd %xmm2, %xmm2
     8:  movl $0x32, %eax
     d:  movupd 0x7b(%rip), %xmm3
    15:  movdqa %xmm2, %xmm4
    19:  movdqa %xmm2, %xmm5
    1d:  movdqa %xmm2, %xmm7
    21:  movdqa %xmm2, %xmm6
    25:  addsd %xmm5, %xmm6
    29:  ucomisd %xmm6, %xmm3
    2d:  jb 0x7e
    33:  subsd %xmm5, %xmm2
    37:  addsd %xmm0, %xmm2
    3b:  movdqa %xmm2, %xmm6
    3f:  mulsd %xmm2, %xmm6
    43:  addsd %xmm4, %xmm4
    47:  mulsd %xmm7, %xmm4
    4b:  addsd %xmm1, %xmm4
    4f:  movdqa %xmm4, %xmm5
    53:  mulsd %xmm4, %xmm5
    57:  subq $1, %rax
    5b:  testq %rax, %rax
    5e:  jg 0x6d
    64:  movdqa %xmm6, %xmm2
    68:  jmp 0x7e
    6d:  movdqa %xmm4, %xmm7
    71:  movdqa %xmm2, %xmm4
    75:  movdqa %xmm6, %xmm2
    79:  jmp 0x21
    7e:  addsd %xmm5, %xmm2
    82:  ucomisd %xmm2, %xmm3
    86:  setae %al
    89:  movq %rbp, %rsp
    8c:  popq %rbp
    8d:  retq
    8e:  addb %al, (%rax)
    90:  addb %al, (%rax)
    92:  addb %al, (%rax)
    94:  addb %al, (%rax)
    96:  adcb %al, (%rax)
    99:  addb %al, (%rax)
    9b:  addb %al, (%rax)
    9d:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %c
function %c(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    jump block2

block3:
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v10 = iconst.i64 1
    jump block4(v4, v4, v4, v4, v9)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50

block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v28 = fadd v26, v27
    v32 = fcmp le v28, v31
    brif v32, block7, block8

block7:
    jump block6

block8:
    jump block1(v26, v27)

block6:
    jump block5

block5:
    v116 = iconst.i64 1
    v117 = isub.i64 v45, v116  ; v116 = 1
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    brif v117, block4(v43, v44, v42, v39, v117), block1(v43, v44)

block2:
    jump block3

block1(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  xorpd %xmm2, %xmm2
     8:  movl $0x32, %eax
     d:  movupd 0x7b(%rip), %xmm3
    15:  movdqa %xmm2, %xmm4
    19:  movdqa %xmm2, %xmm5
    1d:  movdqa %xmm2, %xmm6
    21:  movdqa %xmm2, %xmm7
    25:  addsd %xmm5, %xmm7
    29:  ucomisd %xmm7, %xmm3
    2d:  jb 0x79
    33:  subq $1, %rax
    37:  subsd %xmm5, %xmm2
    3b:  addsd %xmm0, %xmm2
    3f:  movdqa %xmm2, %xmm7
    43:  mulsd %xmm2, %xmm7
    47:  addsd %xmm4, %xmm4
    4b:  mulsd %xmm6, %xmm4
    4f:  addsd %xmm1, %xmm4
    53:  movdqa %xmm4, %xmm5
    57:  mulsd %xmm4, %xmm5
    5b:  testq %rax, %rax
    5e:  je 0x75
    64:  movdqa %xmm4, %xmm6
    68:  movdqa %xmm2, %xmm4
    6c:  movdqa %xmm7, %xmm2
    70:  jmp 0x21
    75:  movdqa %xmm7, %xmm2
    79:  addsd %xmm5, %xmm2
    7d:  ucomisd %xmm2, %xmm3
    81:  setae %al
    84:  movq %rbp, %rsp
    87:  popq %rbp
    88:  retq
    89:  addb %al, (%rax)
    8b:  addb %al, (%rax)
    8d:  addb %al, (%rax)
    8f:  addb %al, (%rax)
    91:  addb %al, (%rax)
    93:  addb %al, (%rax)
    95:  addb %dl, (%rax)
    97:  addb %al, (%rax)
    9a:  addb %al, (%rax)
    9c:  addb %al, (%rax)
    9e:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %d
function %d(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    jump block2

block3:
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v10 = iconst.i64 1
    jump block4(v4, v4, v4, v4, v9)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50

block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v28 = fadd v26, v27
    v32 = fcmp le v28, v31
    brif v32, block7, block8

block7:
    jump block6

block8:
    jump block1(v26, v27)

block6:
    jump block5

block5:
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    v116 = iconst.i64 1
    v117 = isub.i64 v45, v116  ; v116 = 1
    v118 = icmp.i64 eq v45, v116  ; v116 = 1
    brif v118, block1(v43, v44), block4(v43, v44, v42, v39, v117)

block2:
    jump block3

block1(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  xorpd %xmm2, %xmm2
     8:  movl $0x32, %r11d
     e:  movupd 0x8a(%rip), %xmm3
    16:  movdqa %xmm2, %xmm4
    1a:  movdqa %xmm2, %xmm5
    1e:  movdqa %xmm2, %xmm7
    22:  movdqa %xmm2, %xmm6
    26:  addsd %xmm5, %xmm6
    2a:  ucomisd %xmm6, %xmm3
    2e:  jb 0x86
    34:  subsd %xmm5, %xmm2
    38:  addsd %xmm0, %xmm2
    3c:  movdqa %xmm2, %xmm6
    40:  mulsd %xmm2, %xmm6
    44:  addsd %xmm4, %xmm4
    48:  mulsd %xmm7, %xmm4
    4c:  addsd %xmm1, %xmm4
    50:  movdqa %xmm4, %xmm5
    54:  mulsd %xmm4, %xmm5
    58:  movq %r11, %rax
    5b:  subq $1, %rax
    5f:  cmpq $1, %r11
    63:  jne 0x72
    69:  movdqa %xmm6, %xmm2
    6d:  jmp 0x86
    72:  movq %rax, %r11
    75:  movdqa %xmm4, %xmm7
    79:  movdqa %xmm2, %xmm4
    7d:  movdqa %xmm6, %xmm2
    81:  jmp 0x22
    86:  addsd %xmm5, %xmm2
    8a:  ucomisd %xmm2, %xmm3
    8e:  setae %al
    91:  movq %rbp, %rsp
    94:  popq %rbp
    95:  retq
    96:  addb %al, (%rax)
    98:  addb %al, (%rax)
    9a:  addb %al, (%rax)
    9c:  addb %al, (%rax)
    9e:  addb %al, (%rax)
    a0:  addb %al, (%rax)
    a2:  addb %al, (%rax)
    a4:  addb %al, (%rax)
    a6:  adcb %al, (%rax)
    a9:  addb %al, (%rax)
    ab:  addb %al, (%rax)
    ad:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %e
function %e(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    jump block2

block3:
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v10 = iconst.i64 1
    v2 = iconst.i64 0
    jump block4(v4, v4, v4, v4, v9)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50

block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v28 = fadd v26, v27
    v32 = fcmp ugt v28, v31
    brif v32, block8, block7

block7:
    jump block6

block8:
    jump block1(v26, v27)

block6:
    jump block5

block5:
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    v109 = iconst.i64 1
    v110 = isub.i64 v45, v109  ; v109 = 1
    v111 = iconst.i64 0
    v112 = icmp sgt v110, v111  ; v111 = 0
    brif v112, block4(v43, v44, v42, v39, v110), block1(v43, v44)

block2:
    jump block3

block1(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  xorpd %xmm2, %xmm2
     8:  movl $0x32, %eax
     d:  movupd 0x7b(%rip), %xmm3
    15:  movdqa %xmm2, %xmm4
    19:  movdqa %xmm2, %xmm5
    1d:  movdqa %xmm2, %xmm7
    21:  movdqa %xmm2, %xmm6
    25:  addsd %xmm5, %xmm6
    29:  ucomisd %xmm6, %xmm3
    2d:  jb 0x79
    33:  subsd %xmm5, %xmm2
    37:  addsd %xmm0, %xmm2
    3b:  movdqa %xmm2, %xmm6
    3f:  mulsd %xmm2, %xmm6
    43:  addsd %xmm4, %xmm4
    47:  mulsd %xmm7, %xmm4
    4b:  addsd %xmm1, %xmm4
    4f:  movdqa %xmm4, %xmm5
    53:  mulsd %xmm4, %xmm5
    57:  subq $1, %rax
    5b:  testq %rax, %rax
    5e:  jle 0x75
    64:  movdqa %xmm4, %xmm7
    68:  movdqa %xmm2, %xmm4
    6c:  movdqa %xmm6, %xmm2
    70:  jmp 0x21
    75:  movdqa %xmm6, %xmm2
    79:  addsd %xmm5, %xmm2
    7d:  ucomisd %xmm2, %xmm3
    81:  setae %al
    84:  movq %rbp, %rsp
    87:  popq %rbp
    88:  retq
    89:  addb %al, (%rax)
    8b:  addb %al, (%rax)
    8d:  addb %al, (%rax)
    8f:  addb %al, (%rax)
    91:  addb %al, (%rax)
    93:  addb %al, (%rax)
    95:  addb %dl, (%rax)
    97:  addb %al, (%rax)
    9a:  addb %al, (%rax)
    9c:  addb %al, (%rax)
    9e:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %f
function %f(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    jump block2

block3:
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v10 = iconst.i64 1
    v2 = iconst.i64 0
    jump block4(v4, v4, v4, v4, v9)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50

block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v28 = fadd v26, v27
    v32 = fcmp ugt v28, v31
    brif v32, block8, block7

block7:
    jump block6

block8:
    jump block1(v26, v27)

block6:
    jump block5

block5:
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    v109 = iconst.i64 1
    v110 = isub.i64 v45, v109  ; v109 = 1
    v111 = iconst.i64 0
    v112 = icmp sle v110, v111  ; v111 = 0
    brif v112, block1(v43, v44), block4(v43, v44, v42, v39, v110)

block2:
    jump block3

block1(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  xorpd %xmm2, %xmm2
     8:  movl $0x32, %eax
     d:  movupd 0x7b(%rip), %xmm3
    15:  movdqa %xmm2, %xmm4
    19:  movdqa %xmm2, %xmm5
    1d:  movdqa %xmm2, %xmm7
    21:  movdqa %xmm2, %xmm6
    25:  addsd %xmm5, %xmm6
    29:  ucomisd %xmm6, %xmm3
    2d:  jb 0x7e
    33:  subsd %xmm5, %xmm2
    37:  addsd %xmm0, %xmm2
    3b:  movdqa %xmm2, %xmm6
    3f:  mulsd %xmm2, %xmm6
    43:  addsd %xmm4, %xmm4
    47:  mulsd %xmm7, %xmm4
    4b:  addsd %xmm1, %xmm4
    4f:  movdqa %xmm4, %xmm5
    53:  mulsd %xmm4, %xmm5
    57:  subq $1, %rax
    5b:  testq %rax, %rax
    5e:  jg 0x6d
    64:  movdqa %xmm6, %xmm2
    68:  jmp 0x7e
    6d:  movdqa %xmm4, %xmm7
    71:  movdqa %xmm2, %xmm4
    75:  movdqa %xmm6, %xmm2
    79:  jmp 0x21
    7e:  addsd %xmm5, %xmm2
    82:  ucomisd %xmm2, %xmm3
    86:  setae %al
    89:  movq %rbp, %rsp
    8c:  popq %rbp
    8d:  retq
    8e:  addb %al, (%rax)
    90:  addb %al, (%rax)
    92:  addb %al, (%rax)
    94:  addb %al, (%rax)
    96:  adcb %al, (%rax)
    99:  addb %al, (%rax)
    9b:  addb %al, (%rax)
    9d:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %g
function %g(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    v68 = f64const 0.0
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v5 = fcmp le v68, v31  ; v68 = 0.0
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v10 = iconst.i64 1
    v2 = iconst.i64 0
    brif v5, block1(v4, v4, v4, v4, v9), block4(v4, v4)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50, v4 = 0.0, v4 = 0.0

block1(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v69 = iconst.i64 1
    v70 = isub v45, v69  ; v69 = 1
    v71 = iconst.i64 0
    v72 = icmp sle v70, v71  ; v71 = 0
    v40 = fsub v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    brif v72, block4(v43, v44), block2

block2:
    v48 = fadd.f64 v43, v44
    v49 = fcmp le v48, v31
    brif v49, block1(v43, v44, v42, v39, v70), block4(v43, v44)

block4(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  movdqa %xmm0, %xmm4
     8:  movupd 0xa0(%rip), %xmm0
    10:  xorpd %xmm2, %xmm2
    14:  movl $0x32, %eax
    19:  ucomisd 0x9f(%rip), %xmm0
    21:  jb 0x3c
    27:  movdqa %xmm2, %xmm3
    2b:  movdqa %xmm4, %xmm5
    2f:  movdqa %xmm2, %xmm6
    33:  movdqa %xmm2, %xmm7
    37:  jmp 0x49
    3c:  movdqa %xmm2, %xmm4
    40:  movdqa %xmm2, %xmm6
    44:  jmp 0x9d
    49:  subq $1, %rax
    4d:  subsd %xmm6, %xmm2
    51:  addsd %xmm5, %xmm2
    55:  movdqa %xmm2, %xmm4
    59:  mulsd %xmm2, %xmm4
    5d:  addsd %xmm3, %xmm3
    61:  mulsd %xmm7, %xmm3
    65:  addsd %xmm1, %xmm3
    69:  movdqa %xmm3, %xmm6
    6d:  mulsd %xmm3, %xmm6
    71:  testq %rax, %rax
    74:  jle 0x9d
    7a:  movdqa %xmm4, %xmm7
    7e:  addsd %xmm6, %xmm7
    82:  ucomisd %xmm7, %xmm0
    86:  jb 0x9d
    8c:  movdqa %xmm3, %xmm7
    90:  movdqa %xmm2, %xmm3
    94:  movdqa %xmm4, %xmm2
    98:  jmp 0x49
    9d:  addsd %xmm6, %xmm4
    a1:  ucomisd %xmm4, %xmm0
    a5:  setae %al
    a8:  movq %rbp, %rsp
    ab:  popq %rbp
    ac:  retq
    ad:  addb %al, (%rax)
    af:  addb %al, (%rax)
    b1:  addb %al, (%rax)
    b3:  addb %al, (%rax)
    b5:  addb %dl, (%rax)
    b7:  addb %al, (%rax)
    ba:  addb %al, (%rax)
    bc:  addb %al, (%rax)
    be:  addb %al, (%rax)
    c0:  addb %al, (%rax)
    c2:  addb %al, (%rax)
    c4:  addb %al, (%rax)
    c6:  addb %al, (%rax)
    c8:  addb %al, (%rax)
    ca:  addb %al, (%rax)
    cc:  addb %al, (%rax)
    ce:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %h
function %h(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    v68 = f64const 0.0
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v5 = fcmp le v68, v31  ; v68 = 0.0
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v10 = iconst.i64 1
    brif v5, block1(v4, v4, v4, v4, v9), block4(v4, v4)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50, v4 = 0.0, v4 = 0.0

block1(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v76 = iconst.i64 1
    v77 = icmp eq v45, v76  ; v76 = 1
    v40 = fsub v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    brif v77, block4(v43, v44), block2

block2:
    v48 = fadd.f64 v43, v44
    v49 = fcmp le v48, v31
    v78 = iconst.i64 1
    v79 = isub.i64 v45, v78  ; v78 = 1
    brif v49, block1(v43, v44, v42, v39, v79), block4(v43, v44)

block4(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  movdqa %xmm0, %xmm3
     8:  movupd 0xa0(%rip), %xmm0
    10:  xorpd %xmm2, %xmm2
    14:  movl $0x32, %eax
    19:  ucomisd 0x9f(%rip), %xmm0
    21:  jb 0x3c
    27:  movdqa %xmm2, %xmm4
    2b:  movdqa %xmm3, %xmm5
    2f:  movdqa %xmm2, %xmm6
    33:  movdqa %xmm2, %xmm7
    37:  jmp 0x49
    3c:  movdqa %xmm2, %xmm3
    40:  movdqa %xmm2, %xmm6
    44:  jmp 0x9e
    49:  subsd %xmm6, %xmm2
    4d:  addsd %xmm5, %xmm2
    51:  movdqa %xmm2, %xmm3
    55:  mulsd %xmm2, %xmm3
    59:  addsd %xmm4, %xmm4
    5d:  mulsd %xmm7, %xmm4
    61:  addsd %xmm1, %xmm4
    65:  movdqa %xmm4, %xmm6
    69:  mulsd %xmm4, %xmm6
    6d:  cmpq $1, %rax
    71:  je 0x9e
    77:  movdqa %xmm3, %xmm7
    7b:  addsd %xmm6, %xmm7
    7f:  subq $1, %rax
    83:  ucomisd %xmm7, %xmm0
    87:  jb 0x9e
    8d:  movdqa %xmm4, %xmm7
    91:  movdqa %xmm2, %xmm4
    95:  movdqa %xmm3, %xmm2
    99:  jmp 0x49
    9e:  addsd %xmm6, %xmm3
    a2:  ucomisd %xmm3, %xmm0
    a6:  setae %al
    a9:  movq %rbp, %rsp
    ac:  popq %rbp
    ad:  retq
    ae:  addb %al, (%rax)
    b0:  addb %al, (%rax)
    b2:  addb %al, (%rax)
    b4:  addb %al, (%rax)
    b6:  adcb %al, (%rax)
    b9:  addb %al, (%rax)
    bb:  addb %al, (%rax)
    bd:  addb %al, (%rax)
    bf:  addb %al, (%rax)
    c1:  addb %al, (%rax)
    c3:  addb %al, (%rax)
    c5:  addb %al, (%rax)
    c7:  addb %al, (%rax)
    c9:  addb %al, (%rax)
    cb:  addb %al, (%rax)
    cd:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %i
function %i(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    jump block2

block3:
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v10 = iconst.i64 1
    v2 = iconst.i64 0
    jump block4(v4, v4, v4, v4, v9)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50

block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v28 = fadd v26, v27
    v32 = fcmp le v28, v31
    brif v32, block7, block8

block7:
    jump block6

block8:
    return v32

block6:
    jump block5

block5:
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    v203 = iconst.i64 1
    v204 = isub.i64 v45, v203  ; v203 = 1
    v205 = iconst.i64 0
    v206 = icmp sgt v204, v205  ; v205 = 0
    brif v206, block4(v43, v44, v42, v39, v204), block1

block2:
    jump block3

block1:
    v63 = fadd.f64 v43, v44
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  xorpd %xmm2, %xmm2
     8:  movl $0x32, %eax
     d:  movupd 0x8b(%rip), %xmm3
    15:  movdqa %xmm2, %xmm4
    19:  movdqa %xmm2, %xmm5
    1d:  movdqa %xmm2, %xmm6
    21:  movdqa %xmm2, %xmm7
    25:  addsd %xmm5, %xmm7
    29:  ucomisd %xmm7, %xmm3
    2d:  setae %cl
    30:  ucomisd %xmm7, %xmm3
    34:  jae 0x42
    3a:  movq %rcx, %rax
    3d:  movq %rbp, %rsp
    40:  popq %rbp
    41:  retq
    42:  subsd %xmm5, %xmm2
    46:  addsd %xmm0, %xmm2
    4a:  movdqa %xmm2, %xmm7
    4e:  mulsd %xmm2, %xmm7
    52:  addsd %xmm4, %xmm4
    56:  mulsd %xmm6, %xmm4
    5a:  addsd %xmm1, %xmm4
    5e:  movdqa %xmm4, %xmm5
    62:  mulsd %xmm4, %xmm5
    66:  subq $1, %rax
    6a:  testq %rax, %rax
    6d:  jle 0x84
    73:  movdqa %xmm4, %xmm6
    77:  movdqa %xmm2, %xmm4
    7b:  movdqa %xmm7, %xmm2
    7f:  jmp 0x21
    84:  addsd %xmm5, %xmm7
    88:  ucomisd %xmm7, %xmm3
    8c:  setae %al
    8f:  movq %rbp, %rsp
    92:  popq %rbp
    93:  retq
    94:  addb %al, (%rax)
    96:  addb %al, (%rax)
    98:  addb %al, (%rax)
    9a:  addb %al, (%rax)
    9c:  addb %al, (%rax)
    9e:  addb %al, (%rax)
    a0:  addb %al, (%rax)
    a2:  addb %al, (%rax)
    a4:  addb %al, (%rax)
    a6:  adcb %al, (%rax)
    a9:  addb %al, (%rax)
    ab:  addb %al, (%rax)
    ad:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %j
function %j(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    v203 = f64const 0.0
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v5 = fcmp le v203, v31  ; v203 = 0.0
    v4 = f64const 0.0
    v9 = iconst.i64 50
    v10 = iconst.i64 1
    v2 = iconst.i64 0
    brif v5, block1(v4, v4, v4, v4, v9), block5  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = 50

block1(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v204 = iconst.i64 1
    v205 = isub v45, v204  ; v204 = 1
    v206 = iconst.i64 0
    v207 = icmp sle v205, v206  ; v206 = 0
    brif v207, block4, block2

block2:
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    v48 = fadd v43, v44
    v49 = fcmp le v48, v31
    brif v49, block1(v43, v44, v42, v39, v205), block4

block4:
    v208 = fsub.f64 v26, v27
    v209 = fadd v208, v0
    v210 = fmul v209, v209
    v211 = fadd.f64 v34, v34
    v212 = fmul v211, v36
    v213 = fadd v212, v1
    v214 = fmul v213, v213
    v215 = fadd v210, v214
    v216 = fcmp le v215, v31
    return v216

block5:
    return v5
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  movdqa %xmm0, %xmm6
     8:  movupd 0xe0(%rip), %xmm0
    10:  ucomisd 0xe8(%rip), %xmm0
    18:  setae %cl
    1b:  xorpd %xmm2, %xmm2
    1f:  movl $0x32, %eax
    24:  ucomisd 0xe4(%rip), %xmm0
    2c:  jb 0x43
    32:  movdqa %xmm2, %xmm3
    36:  movdqa %xmm2, %xmm4
    3a:  movdqa %xmm2, %xmm5
    3e:  jmp 0x4b
    43:  movq %rcx, %rax
    46:  movq %rbp, %rsp
    49:  popq %rbp
    4a:  retq
    4b:  subq $1, %rax
    4f:  testq %rax, %rax
    52:  jle 0xb9
    58:  movdqa %xmm2, %xmm7
    5c:  subsd %xmm5, %xmm7
    60:  addsd %xmm6, %xmm7
    64:  movdqa %xmm7, %xmm8
    69:  mulsd %xmm7, %xmm8
    6e:  movdqa %xmm3, %xmm9
    73:  addsd %xmm3, %xmm9
    78:  mulsd %xmm4, %xmm9
    7d:  addsd %xmm1, %xmm9
    82:  movdqa %xmm9, %xmm10
    87:  mulsd %xmm9, %xmm10
    8c:  movdqa %xmm8, %xmm11
    91:  addsd %xmm10, %xmm11
    96:  ucomisd %xmm11, %xmm0
    9b:  jb 0xb9
    a1:  movdqa %xmm8, %xmm2
    a6:  movdqa %xmm7, %xmm3
    aa:  movdqa %xmm9, %xmm4
    af:  movdqa %xmm10, %xmm5
    b4:  jmp 0x4b
    b9:  subsd %xmm5, %xmm2
    bd:  addsd %xmm6, %xmm2
    c1:  mulsd %xmm2, %xmm2
    c5:  addsd %xmm3, %xmm3
    c9:  mulsd %xmm4, %xmm3
    cd:  addsd %xmm1, %xmm3
    d1:  mulsd %xmm3, %xmm3
    d5:  addsd %xmm3, %xmm2
    d9:  ucomisd %xmm2, %xmm0
    dd:  setae %al
    e0:  movq %rbp, %rsp
    e3:  popq %rbp
    e4:  retq
    e5:  addb %al, (%rax)
    e7:  addb %al, (%rax)
    e9:  addb %al, (%rax)
    eb:  addb %al, (%rax)
    ed:  addb %al, (%rax)
    ef:  addb %al, (%rax)
    f1:  addb %al, (%rax)
    f3:  addb %al, (%rax)
    f5:  addb %dl, (%rax)
    f7:  addb %al, (%rax)
    fa:  addb %al, (%rax)
    fc:  addb %al, (%rax)
    fe:  addb %al, (%rax)
   100:  addb %al, (%rax)
   102:  addb %al, (%rax)
   104:  addb %al, (%rax)
   106:  addb %al, (%rax)
   108:  addb %al, (%rax)
   10a:  addb %al, (%rax)
   10c:  addb %al, (%rax)
   10e:  addb %al, (%rax)
   110:  addb %al, (%rax)
   112:  addb %al, (%rax)
   114:  addb %al, (%rax)
   116:  addb %al, (%rax)
   118:  addb %al, (%rax)
   11a:  addb %al, (%rax)
   11c:  addb %al, (%rax)
   11e:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %k
function %k(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    jump block2

block3:
    v4 = f64const 0.0
    v9 = iconst.i64 -50
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v10 = iconst.i64 1
    jump block4(v4, v4, v4, v4, v9)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = -50

block4(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v28 = fadd v26, v27
    v32 = fcmp le v28, v31
    brif v32, block7, block8

block7:
    jump block6

block8:
    jump block1(v26, v27)

block6:
    jump block5

block5:
    v109 = iconst.i64 1
    v106, v108 = uadd_overflow.i64 v45, v109  ; v109 = 1
    v40 = fsub.f64 v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd.f64 v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    brif v108, block1(v43, v44), block4(v43, v44, v42, v39, v106)

block2:
    jump block3

block1(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  xorpd %xmm2, %xmm2
     8:  movq $18446744073709551566, %rax
     f:  movupd 0x79(%rip), %xmm3
    17:  movdqa %xmm2, %xmm4
    1b:  movdqa %xmm2, %xmm5
    1f:  movdqa %xmm2, %xmm6
    23:  movdqa %xmm2, %xmm7
    27:  addsd %xmm5, %xmm7
    2b:  ucomisd %xmm7, %xmm3
    2f:  jb 0x7d
    35:  subsd %xmm5, %xmm2
    39:  addsd %xmm0, %xmm2
    3d:  movdqa %xmm2, %xmm7
    41:  mulsd %xmm2, %xmm7
    45:  addsd %xmm4, %xmm4
    49:  mulsd %xmm6, %xmm4
    4d:  addsd %xmm1, %xmm4
    51:  movdqa %xmm4, %xmm5
    55:  mulsd %xmm4, %xmm5
    59:  addq $1, %rax
    5d:  jae 0x6c
    63:  movdqa %xmm7, %xmm2
    67:  jmp 0x7d
    6c:  movdqa %xmm4, %xmm6
    70:  movdqa %xmm2, %xmm4
    74:  movdqa %xmm7, %xmm2
    78:  jmp 0x23
    7d:  addsd %xmm5, %xmm2
    81:  ucomisd %xmm2, %xmm3
    85:  setae %al
    88:  movq %rbp, %rsp
    8b:  popq %rbp
    8c:  retq
    8d:  addb %al, (%rax)
    8f:  addb %al, (%rax)
    91:  addb %al, (%rax)
    93:  addb %al, (%rax)
    95:  addb %dl, (%rax)
    97:  addb %al, (%rax)
    9a:  addb %al, (%rax)
    9c:  addb %al, (%rax)
    9e:  addb %al, (%rax)
==== experiments/kernels/counted-latch/escape.clif %l
function %l(f64, f64) -> i8 tail {
    const0 = 0x00000000000000004010000000000000

block0(v0: f64, v1: f64):
    v68 = f64const 0.0
    v30 = vconst.f64x2 const0
    v31 = extractlane v30, 0  ; v30 = const0
    v5 = fcmp le v68, v31  ; v68 = 0.0
    v4 = f64const 0.0
    v9 = iconst.i64 -50
    v10 = iconst.i64 1
    brif v5, block1(v4, v4, v4, v4, v9), block4(v4, v4)  ; v4 = 0.0, v4 = 0.0, v4 = 0.0, v4 = 0.0, v9 = -50, v4 = 0.0, v4 = 0.0

block1(v26: f64, v27: f64, v34: f64, v36: f64, v45: i64):
    v69 = iconst.i64 1
    v46, v47 = uadd_overflow v45, v69  ; v69 = 1
    v40 = fsub v26, v27
    v42 = fadd v40, v0
    v43 = fmul v42, v42
    v35 = fadd v34, v34
    v37 = fmul v35, v36
    v39 = fadd v37, v1
    v44 = fmul v39, v39
    brif v47, block4(v43, v44), block2

block2:
    v48 = fadd.f64 v43, v44
    v49 = fcmp le v48, v31
    brif v49, block1(v43, v44, v42, v39, v46), block4(v43, v44)

block4(v61: f64, v62: f64):
    v63 = fadd v61, v62
    v67 = fcmp le v63, v31
    return v67
}

     0:  pushq %rbp
     1:  movq %rsp, %rbp
     4:  movdqa %xmm0, %xmm3
     8:  movupd 0xa0(%rip), %xmm0
    10:  xorpd %xmm2, %xmm2
    14:  movq $18446744073709551566, %rax
    1b:  ucomisd 0x9d(%rip), %xmm0
    23:  jb 0x3e
    29:  movdqa %xmm2, %xmm4
    2d:  movdqa %xmm3, %xmm5
    31:  movdqa %xmm2, %xmm6
    35:  movdqa %xmm2, %xmm7
    39:  jmp 0x4b
    3e:  movdqa %xmm2, %xmm3
    42:  movdqa %xmm2, %xmm6
    46:  jmp 0x9c
    4b:  subsd %xmm6, %xmm2
    4f:  addsd %xmm5, %xmm2
    53:  movdqa %xmm2, %xmm3
    57:  mulsd %xmm2, %xmm3
    5b:  addsd %xmm4, %xmm4
    5f:  mulsd %xmm7, %xmm4
    63:  addsd %xmm1, %xmm4
    67:  movdqa %xmm4, %xmm6
    6b:  mulsd %xmm4, %xmm6
    6f:  addq $1, %rax
    73:  jb 0x9c
    79:  movdqa %xmm3, %xmm7
    7d:  addsd %xmm6, %xmm7
    81:  ucomisd %xmm7, %xmm0
    85:  jb 0x9c
    8b:  movdqa %xmm4, %xmm7
    8f:  movdqa %xmm2, %xmm4
    93:  movdqa %xmm3, %xmm2
    97:  jmp 0x4b
    9c:  addsd %xmm6, %xmm3
    a0:  ucomisd %xmm3, %xmm0
    a4:  setae %al
    a7:  movq %rbp, %rsp
    aa:  popq %rbp
    ab:  retq
    ac:  addb %al, (%rax)
    ae:  addb %al, (%rax)
    b0:  addb %al, (%rax)
    b2:  addb %al, (%rax)
    b4:  addb %al, (%rax)
    b6:  adcb %al, (%rax)
    b9:  addb %al, (%rax)
    bb:  addb %al, (%rax)
    bd:  addb %al, (%rax)
    bf:  addb %al, (%rax)
    c1:  addb %al, (%rax)
    c3:  addb %al, (%rax)
    c5:  addb %al, (%rax)
    c7:  addb %al, (%rax)
    c9:  addb %al, (%rax)
    cb:  addb %al, (%rax)
    cd:  addb %al, (%rax)
