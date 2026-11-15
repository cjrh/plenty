0000000000210738 <__plenty_fn_transform_forward>:
  210738:	push   %rbp
  210739:	mov    %rsp,%rbp
  21073c:	sub    $0x80,%rsp
  210743:	mov    %rbx,0x50(%rsp)
  210748:	mov    %r12,0x58(%rsp)
  21074d:	mov    %r13,0x60(%rsp)
  210752:	mov    %r14,0x68(%rsp)
  210757:	mov    %r15,0x70(%rsp)
  21075c:	mov    %rdi,0x10(%rsp)
  210761:	mov    %rdx,0x20(%rsp)
  210766:	mov    $0x1,%eax
  21076b:	mov    %rax,0x40(%rsp)
  210770:	cmp    $0x2,%rsi
  210774:	mov    %rsi,0x18(%rsp)
  210779:	jl     2109c3 <__plenty_fn_transform_forward+0x28b>
  21077f:	mov    0x18(%rsp),%rax
  210784:	mov    %rax,0x30(%rsp)
  210789:	xor    %r9,%r9
  21078c:	mov    0x30(%rsp),%rsi
  210791:	mov    %r9,0x28(%rsp)
  210796:	mov    %rsi,%rdx
  210799:	sar    $1,%rdx
  21079c:	mov    0x18(%rsp),%r10
  2107a1:	mov    %rdx,0x48(%rsp)
  2107a6:	test   %r10,%r10
  2107a9:	jg     2107be <__plenty_fn_transform_forward+0x86>
  2107af:	mov    0x40(%rsp),%rax
  2107b4:	mov    0x20(%rsp),%rbx
  2107b9:	jmp    210993 <__plenty_fn_transform_forward+0x25b>
  2107be:	mov    0x30(%rsp),%rsi
  2107c3:	mov    0x28(%rsp),%r9
  2107c8:	mov    %r9,%r11
  2107cb:	add    %rsi,%r11
  2107ce:	mov    %r9,0x28(%rsp)
  2107d3:	jo     210b53 <__plenty_fn_transform_forward+0x41b>
  2107d9:	mov    0x18(%rsp),%r10
  2107de:	cmp    %r10,%r11
  2107e1:	ja     2109e8 <__plenty_fn_transform_forward+0x2b0>
  2107e7:	mov    0x28(%rsp),%r9
  2107ec:	cmp    %r11,%r9
  2107ef:	ja     2109e8 <__plenty_fn_transform_forward+0x2b0>
  2107f5:	mov    0x28(%rsp),%r9
  2107fa:	sub    %r9,%r11
  2107fd:	mov    0x28(%rsp),%rax
  210802:	mov    0x10(%rsp),%r9
  210807:	lea    (%r9,%rax,8),%rdi
  21080b:	mov    0x48(%rsp),%r14
  210810:	mov    %r9,0x10(%rsp)
  210815:	cmp    %r11,%r14
  210818:	ja     210a4e <__plenty_fn_transform_forward+0x316>
  21081e:	xor    %rbx,%rbx
  210821:	mov    %r14,%r12
  210824:	cmp    %r11,%r14
  210827:	ja     210a79 <__plenty_fn_transform_forward+0x341>
  21082d:	sub    %r14,%r11
  210830:	mov    %r14,0x48(%rsp)
  210835:	mov    %r14,%rsi
  210838:	lea    (%rdi,%rbx,8),%rdx
  21083c:	xor    %r13,%r13
  21083f:	lea    (%rdi,%rsi,8),%r14
  210843:	test   %r12,%r12
  210846:	jg     21085b <__plenty_fn_transform_forward+0x123>
  21084c:	mov    0x40(%rsp),%rax
  210851:	mov    0x20(%rsp),%rbx
  210856:	jmp    21094f <__plenty_fn_transform_forward+0x217>
  21085b:	mov    0x20(%rsp),%rbx
  210860:	mov    %r13,%r15
  210863:	mov    0x40(%rsp),%rax
  210868:	mov    %rax,0x38(%rsp)
  21086d:	mov    (%rdx,%r13,8),%rdi
  210871:	cmp    %r11,%r13
  210874:	jae    210aac <__plenty_fn_transform_forward+0x374>
  21087a:	mov    (%r14,%r13,8),%rsi
  21087e:	mov    %rdi,%rax
  210881:	add    %rsi,%rax
  210884:	jb     210b55 <__plenty_fn_transform_forward+0x41d>
  21088a:	add    %r8,%rdi
  21088d:	jb     210b57 <__plenty_fn_transform_forward+0x41f>
  210893:	sub    %rsi,%rdi
  210896:	jb     210b59 <__plenty_fn_transform_forward+0x421>
  21089c:	cmp    %r8,%rax
  21089f:	jb     2108ae <__plenty_fn_transform_forward+0x176>
  2108a5:	sub    %r8,%rax
  2108a8:	jb     210b5b <__plenty_fn_transform_forward+0x423>
  2108ae:	mov    %rax,(%rdx,%r13,8)
  2108b2:	cmp    %r8,%rdi
  2108b5:	jb     2108c4 <__plenty_fn_transform_forward+0x18c>
  2108bb:	sub    %r8,%rdi
  2108be:	jb     210b5d <__plenty_fn_transform_forward+0x425>
  2108c4:	lea    (%rbx,%r15,8),%rax
  2108c8:	cmp    %rcx,%r15
  2108cb:	jae    210acb <__plenty_fn_transform_forward+0x393>
  2108d1:	mov    (%rax),%rax
  2108d4:	mov    %rcx,%rsi
  2108d7:	shr    $1,%rsi
  2108da:	add    %r15,%rsi
  2108dd:	jo     210b5f <__plenty_fn_transform_forward+0x427>
  2108e3:	lea    (%rbx,%rsi,8),%r10
  2108e7:	cmp    %rcx,%rsi
  2108ea:	jae    210afe <__plenty_fn_transform_forward+0x3c6>
  2108f0:	mov    (%r10),%r10
  2108f3:	mov    %rdi,%rsi
  2108f6:	imul   %rax,%rsi
  2108fa:	imul   %r10,%rdi
  2108fe:	shr    $0x20,%rdi
  210902:	imul   %r8,%rdi
  210906:	sub    %rdi,%rsi
  210909:	cmp    %r8,%rsi
  21090c:	jb     210915 <__plenty_fn_transform_forward+0x1dd>
  210912:	sub    %r8,%rsi
  210915:	cmp    %r11,%r13
  210918:	jae    210b31 <__plenty_fn_transform_forward+0x3f9>
  21091e:	mov    %rsi,(%r14,%r13,8)
  210922:	mov    0x40(%rsp),%rax
  210927:	add    %rax,%r15
  21092a:	jo     210b61 <__plenty_fn_transform_forward+0x429>
  210930:	add    $0x1,%r13
  210937:	cmp    %r12,%r13
  21093a:	jge    21094a <__plenty_fn_transform_forward+0x212>
  210940:	mov    %rax,0x40(%rsp)
  210945:	jmp    21086d <__plenty_fn_transform_forward+0x135>
  21094a:	mov    0x38(%rsp),%rax
  21094f:	mov    0x30(%rsp),%rsi
  210954:	mov    0x28(%rsp),%r9
  210959:	add    %rsi,%r9
  21095c:	mov    %r9,0x28(%rsp)
  210961:	jo     210b63 <__plenty_fn_transform_forward+0x42b>
  210967:	mov    0x28(%rsp),%r9
  21096c:	mov    0x18(%rsp),%r10
  210971:	cmp    %r10,%r9
  210974:	jge    210993 <__plenty_fn_transform_forward+0x25b>
  21097a:	mov    0x30(%rsp),%rsi
  21097f:	mov    0x28(%rsp),%r9
  210984:	mov    %rbx,0x20(%rsp)
  210989:	mov    %rax,0x40(%rsp)
  21098e:	jmp    2107c8 <__plenty_fn_transform_forward+0x90>
  210993:	imulq  0x1ce(%rip)        # 210b68 <__plenty_fn_transform_forward+0x430>
  21099a:	jo     210b65 <__plenty_fn_transform_forward+0x42d>
  2109a0:	mov    0x48(%rsp),%r14
  2109a5:	cmp    $0x2,%r14
  2109a9:	jl     2109c3 <__plenty_fn_transform_forward+0x28b>
  2109af:	mov    %rbx,0x20(%rsp)
  2109b4:	mov    %r14,0x30(%rsp)
  2109b9:	mov    %rax,0x40(%rsp)
  2109be:	jmp    210789 <__plenty_fn_transform_forward+0x51>
  2109c3:	mov    0x50(%rsp),%rbx
  2109c8:	mov    0x58(%rsp),%r12
  2109cd:	mov    0x60(%rsp),%r13
  2109d2:	mov    0x68(%rsp),%r14
  2109d7:	mov    0x70(%rsp),%r15
  2109dc:	add    $0x80,%rsp
  2109e3:	mov    %rbp,%rsp
  2109e6:	pop    %rbp
  2109e7:	ret
  2109e8:	xor    %rdi,%rdi
  2109eb:	mov    0x28(%rsp),%r9
  2109f0:	mov    0x18(%rsp),%r10
  2109f5:	lea    (%r9,%r10,1),%rsi
  2109f9:	cmp    %rsi,%rdi
  2109fc:	cmovge %rdi,%rsi
  210a00:	cmp    %r9,%r10
  210a03:	mov    %r9,%rax
  210a06:	cmovl  %r10,%rax
  210a0a:	mov    0x28(%rsp),%rdx
  210a0f:	test   %rdx,%r9
  210a12:	cmovl  %rsi,%rax
  210a16:	lea    (%r11,%r10,1),%rsi
  210a1a:	cmp    %rsi,%rdi
  210a1d:	cmovge %rdi,%rsi
  210a21:	cmp    %r11,%r10
  210a24:	mov    0x18(%rsp),%rdx
  210a29:	mov    %r11,%r10
  210a2c:	cmovl  %rdx,%r10
  210a30:	test   %r11,%r11
  210a33:	cmovl  %rsi,%r10
  210a37:	sub    %rax,%r10
  210a3a:	cmp    %r10,%rdi
  210a3d:	cmovge %rdi,%r10
  210a41:	mov    0x10(%rsp),%r9
  210a46:	mov    %r10,%r11
  210a49:	jmp    210807 <__plenty_fn_transform_forward+0xcf>
  210a4e:	xor    %rbx,%rbx
  210a51:	lea    (%r14,%r11,1),%rax
  210a55:	cmp    %rax,%rbx
  210a58:	cmovge %rbx,%rax
  210a5c:	cmp    %r14,%r11
  210a5f:	mov    %r14,%r12
  210a62:	cmovl  %r11,%r12
  210a66:	test   %r14,%r14
  210a69:	cmovl  %rax,%r12
  210a6d:	cmp    %r12,%rbx
  210a70:	cmovge %rbx,%r12
  210a74:	jmp    210824 <__plenty_fn_transform_forward+0xec>
  210a79:	lea    (%r14,%r11,1),%r13
  210a7d:	xor    %rax,%rax
  210a80:	cmp    %r13,%rax
  210a83:	cmovge %rax,%r13
  210a87:	cmp    %r14,%r11
  210a8a:	mov    %r14,%rsi
  210a8d:	cmovl  %r11,%rsi
  210a91:	test   %r14,%r14
  210a94:	cmovl  %r13,%rsi
  210a98:	sub    %rsi,%r11
  210a9b:	cmp    %r11,%rax
  210a9e:	cmovge %rax,%r11
  210aa2:	mov    %r14,0x48(%rsp)
  210aa7:	jmp    210838 <__plenty_fn_transform_forward+0x100>
  210aac:	lea    (%rsp),%rdi
  210ab0:	mov    %r13,(%rsp)
  210ab4:	mov    %r11,0x8(%rsp)
  210ab9:	lea    -0x9b38(%rip),%rax        # 206f88 <.Ldata1>
  210ac0:	lea    0x10(%rax),%rsi
  210ac4:	call   22eec0 <plenty_trap_list_index>
  210ac9:	ud2
  210acb:	lea    (%r15,%rcx,1),%r10
  210acf:	lea    (%rbx,%r10,8),%rax
  210ad3:	cmp    %rcx,%r10
  210ad6:	jb     2108d1 <__plenty_fn_transform_forward+0x199>
  210adc:	mov    %r15,%rax
  210adf:	lea    (%rsp),%rdi
  210ae3:	mov    %rax,(%rsp)
  210ae7:	mov    %rcx,0x8(%rsp)
  210aec:	lea    -0x9b6b(%rip),%rax        # 206f88 <.Ldata1>
  210af3:	lea    0x60(%rax),%rsi
  210af7:	call   22eec0 <plenty_trap_list_index>
  210afc:	ud2
  210afe:	lea    (%rsi,%rcx,1),%r9
  210b02:	lea    (%rbx,%r9,8),%r10
  210b06:	cmp    %rcx,%r9
  210b09:	jb     2108f0 <__plenty_fn_transform_forward+0x1b8>
  210b0f:	lea    (%rsp),%rdi
  210b13:	mov    %rsi,(%rsp)
  210b17:	mov    %rcx,0x8(%rsp)
  210b1c:	lea    -0x9b9b(%rip),%rax        # 206f88 <.Ldata1>
  210b23:	lea    0x80(%rax),%rsi
  210b2a:	call   22eec0 <plenty_trap_list_index>
  210b2f:	ud2
  210b31:	lea    (%rsp),%rdi
  210b35:	mov    %r13,(%rsp)
  210b39:	mov    %r11,0x8(%rsp)
  210b3e:	lea    -0x9bbd(%rip),%rax        # 206f88 <.Ldata1>
  210b45:	lea    0x90(%rax),%rsi
  210b4c:	call   22eec0 <plenty_trap_list_index>
  210b51:	ud2
  210b53:	ud2
  210b55:	ud2
  210b57:	ud2
  210b59:	ud2
  210b5b:	ud2
  210b5d:	ud2
  210b5f:	ud2
  210b61:	ud2
  210b63:	ud2
  210b65:	ud2
  210b67:	add    %al,(%rdx)
  210b69:	add    %al,(%rax)
  210b6b:	add    %al,(%rax)
  210b6d:	add    %al,(%rax)
	...
