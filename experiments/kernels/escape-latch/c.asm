
target/issue-977-escape/c-kernels.o:     file format elf64-x86-64


Disassembly of section .text:

0000000000000000 <unused>:
   0:	test   %rdi,%rdi
   3:	jle    31 <unused+0x31>
   5:	movsd  0x0(%rip),%xmm2        # d <unused+0xd>
   d:	movsd  0x0(%rip),%xmm1        # 15 <unused+0x15>
  15:	xor    %eax,%eax
  17:	nopw   0x0(%rax,%rax,1)
  20:	mulsd  %xmm2,%xmm0
  24:	add    $0x1,%rax
  28:	addsd  %xmm1,%xmm0
  2c:	cmp    %rax,%rdi
  2f:	jne    20 <unused+0x20>
  31:	ret
  32:	nopl   (%rax)
  35:	data16 cs nopw 0x0(%rax,%rax,1)

0000000000000040 <escape>:
  40:	pxor   %xmm4,%xmm4
  44:	movapd %xmm0,%xmm7
  48:	mov    $0x32,%eax
  4d:	movsd  0x0(%rip),%xmm6        # 55 <escape+0x15>
  55:	movapd %xmm4,%xmm3
  59:	movapd %xmm4,%xmm2
  5d:	movapd %xmm4,%xmm0
  61:	nopw   0x0(%rax,%rax,1)
  6a:	data16 cs nopw 0x0(%rax,%rax,1)
  75:	data16 cs nopw 0x0(%rax,%rax,1)
  80:	addsd  %xmm0,%xmm0
  84:	subsd  %xmm4,%xmm3
  88:	mulsd  %xmm0,%xmm2
  8c:	addsd  %xmm7,%xmm3
  90:	movapd %xmm3,%xmm0
  94:	mulsd  %xmm3,%xmm3
  98:	addsd  %xmm1,%xmm2
  9c:	movapd %xmm2,%xmm4
  a0:	mulsd  %xmm2,%xmm4
  a4:	movapd %xmm3,%xmm5
  a8:	addsd  %xmm4,%xmm5
  ac:	sub    $0x1,%rax
  b0:	je     b8 <escape+0x78>
  b2:	comisd %xmm5,%xmm6
  b6:	jae    80 <escape+0x40>
  b8:	comisd %xmm5,%xmm6
  bc:	setae  %al
  bf:	ret
