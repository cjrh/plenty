
target/perf987/kernel-shoup-c:     file format elf64-x86-64


Disassembly of section .init:

000000000040033c <_init>:
  40033c:	endbr64
  400340:	sub    $0x8,%rsp
  400344:	mov    0x2c95(%rip),%rax        # 402fe0 <__gmon_start__>
  40034b:	test   %rax,%rax
  40034e:	je     400352 <_init+0x16>
  400350:	call   *%rax
  400352:	add    $0x8,%rsp
  400356:	ret

Disassembly of section .plt:

0000000000400360 <free@plt-0x10>:
  400360:	push   0x2c8a(%rip)        # 402ff0 <_GLOBAL_OFFSET_TABLE_+0x8>
  400366:	jmp    *0x2c8c(%rip)        # 402ff8 <_GLOBAL_OFFSET_TABLE_+0x10>
  40036c:	nopl   0x0(%rax)

0000000000400370 <free@plt>:
  400370:	jmp    *0x2c8a(%rip)        # 403000 <free@GLIBC_2.2.5>
  400376:	push   $0x0
  40037b:	jmp    400360 <_init+0x24>

0000000000400380 <abort@plt>:
  400380:	jmp    *0x2c82(%rip)        # 403008 <abort@GLIBC_2.2.5>
  400386:	push   $0x1
  40038b:	jmp    400360 <_init+0x24>

0000000000400390 <__isoc23_strtoull@plt>:
  400390:	jmp    *0x2c7a(%rip)        # 403010 <__isoc23_strtoull@GLIBC_2.38>
  400396:	push   $0x2
  40039b:	jmp    400360 <_init+0x24>

00000000004003a0 <printf@plt>:
  4003a0:	jmp    *0x2c72(%rip)        # 403018 <printf@GLIBC_2.2.5>
  4003a6:	push   $0x3
  4003ab:	jmp    400360 <_init+0x24>

00000000004003b0 <calloc@plt>:
  4003b0:	jmp    *0x2c6a(%rip)        # 403020 <calloc@GLIBC_2.2.5>
  4003b6:	push   $0x4
  4003bb:	jmp    400360 <_init+0x24>

Disassembly of section .text:

00000000004003c0 <main>:
  4003c0:	cmp    $0x2,%edi
  4003c3:	je     4003cb <main+0xb>
  4003c5:	mov    $0x1,%eax
  4003ca:	ret
  4003cb:	sub    $0x58,%rsp
  4003cf:	mov    0x8(%rsi),%rdi
  4003d3:	mov    $0xa,%edx
  4003d8:	xor    %esi,%esi
  4003da:	mov    %rbx,0x28(%rsp)
  4003df:	mov    %rbp,0x30(%rsp)
  4003e4:	mov    %r12,0x38(%rsp)
  4003e9:	call   400390 <__isoc23_strtoull@plt>
  4003ee:	mov    $0x8,%esi
  4003f3:	mov    $0x2000,%edi
  4003f8:	mov    %rax,%r12
  4003fb:	call   4003b0 <calloc@plt>
  400400:	mov    $0x8,%esi
  400405:	mov    $0x2000,%edi
  40040a:	mov    %rax,%rbp
  40040d:	call   4003b0 <calloc@plt>
  400412:	mov    %rax,%rbx
  400415:	test   %rbp,%rbp
  400418:	je     400642 <main+0x282>
  40041e:	xor    %eax,%eax
  400420:	test   %rbx,%rbx
  400423:	je     400642 <main+0x282>
  400429:	nop
  40042a:	data16 cs nopw 0x0(%rax,%rax,1)
  400435:	data16 cs nopw 0x0(%rax,%rax,1)
  400440:	mov    %rax,0x0(%rbp,%rax,8)
  400445:	lea    0x1(%rax),%rdx
  400449:	add    $0x2,%rax
  40044d:	mov    %rdx,0x0(%rbp,%rdx,8)
  400452:	cmp    $0x2000,%rax
  400458:	jne    400440 <main+0x80>
  40045a:	xor    %eax,%eax
  40045c:	nopl   0x0(%rax)
  400460:	cmp    $0xfff,%rax
  400466:	ja     40047c <main+0xbc>
  400468:	movq   $0x1,(%rbx,%rax,8)
  400470:	add    $0x1,%rax
  400474:	cmp    $0xfff,%rax
  40047a:	jbe    400468 <main+0xa8>
  40047c:	movq   $0x4,(%rbx,%rax,8)
  400484:	add    $0x1,%rax
  400488:	cmp    $0x2000,%rax
  40048e:	jne    400460 <main+0xa0>
  400490:	lea    -0x8(%rbp),%rax
  400494:	test   %r12,%r12
  400497:	je     40060c <main+0x24c>
  40049d:	mov    %r13,0x40(%rsp)
  4004a2:	xor    %ecx,%ecx
  4004a4:	mov    %rbp,%rdx
  4004a7:	mov    %r14,0x48(%rsp)
  4004ac:	mov    %r15,0x50(%rsp)
  4004b1:	mov    %rcx,0x18(%rsp)
  4004b6:	mov    $0x2000,%r9d
  4004bc:	mov    $0xd,%edi
  4004c1:	mov    %r12,%r8
  4004c4:	mov    $0x1,%r11d
  4004ca:	nopw   0x0(%rax,%rax,1)
  4004d0:	mov    %r9,%r14
  4004d3:	shr    $1,%r9
  4004d6:	mov    %rdx,%r12
  4004d9:	xor    %esi,%esi
  4004db:	lea    0x0(,%r14,8),%r13
  4004e3:	lea    (%rax,%r9,8),%rbp
  4004e7:	mov    %rax,%r15
  4004ea:	nopw   0x0(%rax,%rax,1)
  4004f0:	add    %r14,%rsi
  4004f3:	cmp    $0x2000,%rsi
  4004fa:	ja     400649 <main+0x289>
  400500:	mov    %r13,0x8(%rsp)
  400505:	mov    %r12,%rcx
  400508:	xor    %r10d,%r10d
  40050b:	mov    %rsi,0x10(%rsp)
  400510:	jmp    40052c <main+0x16c>
  400512:	nopw   0x0(%rax,%rax,1)
  400518:	add    %r11,%r10
  40051b:	add    $0x8,%rcx
  40051f:	cmp    $0xfff,%r10
  400526:	ja     400649 <main+0x289>
  40052c:	mov    (%rcx),%rax
  40052f:	mov    (%rcx,%r9,8),%rsi
  400533:	mov    %rax,%r13
  400536:	add    %rsi,%r13
  400539:	jb     400649 <main+0x289>
  40053f:	add    $0x3b800001,%rax
  400545:	jb     400649 <main+0x289>
  40054b:	sub    %rsi,%rax
  40054e:	jb     400649 <main+0x289>
  400554:	cmp    $0x3b800000,%r13
  40055b:	lea    -0x3b800001(%r13),%rsi
  400562:	cmovbe %r13,%rsi
  400566:	cmp    $0x3b800000,%rax
  40056c:	mov    %rsi,(%rcx)
  40056f:	lea    -0x3b800001(%rax),%rsi
  400576:	cmova  %rsi,%rax
  40057a:	mov    (%rbx,%r10,8),%rsi
  40057e:	imul   %rax,%rsi
  400582:	imul   0x8000(%rbx,%r10,8),%rax
  40058b:	shr    $0x20,%rax
  40058f:	imul   $0x3b800001,%rax,%rax
  400596:	sub    %rax,%rsi
  400599:	lea    -0x3b800001(%rsi),%rax
  4005a0:	cmp    $0x3b800000,%rsi
  4005a7:	cmova  %rax,%rsi
  4005ab:	mov    %rsi,(%rcx,%r9,8)
  4005af:	cmp    %rbp,%rcx
  4005b2:	jne    400518 <main+0x158>
  4005b8:	mov    0x8(%rsp),%r13
  4005bd:	mov    0x10(%rsp),%rsi
  4005c2:	lea    (%rcx,%r13,1),%rbp
  4005c6:	add    %r13,%r12
  4005c9:	cmp    $0x2000,%rsi
  4005d0:	jne    4004f0 <main+0x130>
  4005d6:	mov    %r15,%rax
  4005d9:	add    %r11,%r11
  4005dc:	sub    $0x1,%edi
  4005df:	jne    4004d0 <main+0x110>
  4005e5:	mov    0x18(%rsp),%rcx
  4005ea:	mov    %r8,%r12
  4005ed:	add    $0x1,%rcx
  4005f1:	cmp    %rcx,%r8
  4005f4:	jne    4004b1 <main+0xf1>
  4005fa:	mov    0x40(%rsp),%r13
  4005ff:	mov    0x48(%rsp),%r14
  400604:	mov    %rdx,%rbp
  400607:	mov    0x50(%rsp),%r15
  40060c:	mov    0x0(%rbp),%rsi
  400610:	mov    $0x401288,%edi
  400615:	xor    %eax,%eax
  400617:	call   4003a0 <printf@plt>
  40061c:	mov    %rbp,%rdi
  40061f:	call   400370 <free@plt>
  400624:	mov    %rbx,%rdi
  400627:	call   400370 <free@plt>
  40062c:	xor    %eax,%eax
  40062e:	mov    0x28(%rsp),%rbx
  400633:	mov    0x30(%rsp),%rbp
  400638:	mov    0x38(%rsp),%r12
  40063d:	add    $0x58,%rsp
  400641:	ret
  400642:	mov    $0x1,%eax
  400647:	jmp    40062e <main+0x26e>
  400649:	call   400380 <abort@plt>
  40064e:	xchg   %ax,%ax

0000000000400650 <_start>:
  400650:	endbr64
  400654:	xor    %ebp,%ebp
  400656:	mov    %rdx,%r9
  400659:	pop    %rsi
  40065a:	mov    %rsp,%rdx
  40065d:	and    $0xfffffffffffffff0,%rsp
  400661:	push   %rax
  400662:	push   %rsp
  400663:	xor    %r8d,%r8d
  400666:	xor    %ecx,%ecx
  400668:	mov    $0x4003c0,%rdi
  40066f:	call   *0x2963(%rip)        # 402fd8 <__libc_start_main@GLIBC_2.34>
  400675:	hlt
  400676:	cs nopw 0x0(%rax,%rax,1)

0000000000400680 <_dl_relocate_static_pie>:
  400680:	endbr64
  400684:	ret
  400685:	cs nopw 0x0(%rax,%rax,1)
  40068f:	nop

0000000000400690 <deregister_tm_clones>:
  400690:	mov    $0x403030,%eax
  400695:	cmp    $0x403030,%rax
  40069b:	je     4006b0 <deregister_tm_clones+0x20>
  40069d:	mov    $0x0,%eax
  4006a2:	test   %rax,%rax
  4006a5:	je     4006b0 <deregister_tm_clones+0x20>
  4006a7:	mov    $0x403030,%edi
  4006ac:	jmp    *%rax
  4006ae:	xchg   %ax,%ax
  4006b0:	ret
  4006b1:	nopl   0x0(%rax)
  4006b5:	data16 cs nopw 0x0(%rax,%rax,1)

00000000004006c0 <register_tm_clones>:
  4006c0:	mov    $0x403030,%esi
  4006c5:	sub    $0x403030,%rsi
  4006cc:	mov    %rsi,%rax
  4006cf:	shr    $0x3f,%rsi
  4006d3:	sar    $0x3,%rax
  4006d7:	add    %rax,%rsi
  4006da:	sar    $1,%rsi
  4006dd:	je     4006f0 <register_tm_clones+0x30>
  4006df:	mov    $0x0,%eax
  4006e4:	test   %rax,%rax
  4006e7:	je     4006f0 <register_tm_clones+0x30>
  4006e9:	mov    $0x403030,%edi
  4006ee:	jmp    *%rax
  4006f0:	ret
  4006f1:	nopl   0x0(%rax)
  4006f5:	data16 cs nopw 0x0(%rax,%rax,1)

0000000000400700 <__do_global_dtors_aux>:
  400700:	endbr64
  400704:	cmpb   $0x0,0x2921(%rip)        # 40302c <completed.0>
  40070b:	jne    400720 <__do_global_dtors_aux+0x20>
  40070d:	push   %rbp
  40070e:	mov    %rsp,%rbp
  400711:	call   400690 <deregister_tm_clones>
  400716:	movb   $0x1,0x290f(%rip)        # 40302c <completed.0>
  40071d:	pop    %rbp
  40071e:	ret
  40071f:	nop
  400720:	ret
  400721:	nopl   0x0(%rax)
  400725:	data16 cs nopw 0x0(%rax,%rax,1)

0000000000400730 <frame_dummy>:
  400730:	endbr64
  400734:	jmp    4006c0 <register_tm_clones>

Disassembly of section .fini:

0000000000400738 <_fini>:
  400738:	endbr64
  40073c:	sub    $0x8,%rsp
  400740:	add    $0x8,%rsp
  400744:	ret
