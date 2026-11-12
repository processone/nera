
/tmp/nera-binary-list-etf-evidence/before:	file format mach-o arm64

Disassembly of section __TEXT,__text:

000000010016a768 <_nera_rt_binary_to_list>:
10016a768: a9bd57f6    	stp	x22, x21, [sp, #-0x30]!
10016a76c: a9014ff4    	stp	x20, x19, [sp, #0x10]
10016a770: a9027bfd    	stp	x29, x30, [sp, #0x20]
10016a774: 910083fd    	add	x29, sp, #0x20
10016a778: aa0003f4    	mov	x20, x0
10016a77c: f240083f    	tst	x1, #0x7
10016a780: 54000201    	b.ne	0x10016a7c0 <_nera_rt_binary_to_list+0x58>
10016a784: 9240f828    	and	x8, x1, #0x7fffffffffffffff
10016a788: b40001c8    	cbz	x8, 0x10016a7c0 <_nera_rt_binary_to_list+0x58>
10016a78c: 927dec29    	and	x9, x1, #0x7ffffffffffffff8
10016a790: f9400128    	ldr	x8, [x9]
10016a794: 92400d0a    	and	x10, x8, #0xf
10016a798: f100195f    	cmp	x10, #0x6
10016a79c: 54000280    	b.eq	0x10016a7ec <_nera_rt_binary_to_list+0x84>
10016a7a0: f1000d5f    	cmp	x10, #0x3
10016a7a4: 540000e1    	b.ne	0x10016a7c0 <_nera_rt_binary_to_list+0x58>
10016a7a8: 91002133    	add	x19, x9, #0x8
10016a7ac: d347fd09    	lsr	x9, x8, #7
10016a7b0: f27c091f    	tst	x8, #0x70
10016a7b4: 9a890535    	cinc	x21, x9, ne
10016a7b8: b5000294    	cbnz	x20, 0x10016a808 <_nera_rt_binary_to_list+0xa0>
10016a7bc: 14000019    	b	0x10016a820 <_nera_rt_binary_to_list+0xb8>
10016a7c0: aa1403e0    	mov	x0, x20
10016a7c4: 97ffcf9f    	bl	0x10015e640 <__RNvNtNtCs3JpjsLPrFbG_12nera_runtime3ffi7helpers23capture_exception_trace>
10016a7c8: b40000f4    	cbz	x20, 0x10016a7e4 <_nera_rt_binary_to_list+0x7c>
10016a7cc: 52800068    	mov	w8, #0x3                ; =3
10016a7d0: b9001288    	str	w8, [x20, #0x10]
10016a7d4: 52800668    	mov	w8, #0x33               ; =51
10016a7d8: f9000e88    	str	x8, [x20, #0x18]
10016a7dc: 52800028    	mov	w8, #0x1                ; =1
10016a7e0: b9003e88    	str	w8, [x20, #0x3c]
10016a7e4: 528000a0    	mov	w0, #0x5                ; =5
10016a7e8: 14000021    	b	0x10016a86c <_nera_rt_binary_to_list+0x104>
10016a7ec: a940a52a    	ldp	x10, x9, [x9, #0x8]
10016a7f0: f940014a    	ldr	x10, [x10]
10016a7f4: 8b090153    	add	x19, x10, x9
10016a7f8: d347fd09    	lsr	x9, x8, #7
10016a7fc: f27c091f    	tst	x8, #0x70
10016a800: 9a890535    	cinc	x21, x9, ne
10016a804: b40000f4    	cbz	x20, 0x10016a820 <_nera_rt_binary_to_list+0xb8>
10016a808: f9401682    	ldr	x2, [x20, #0x28]
10016a80c: b40000a2    	cbz	x2, 0x10016a820 <_nera_rt_binary_to_list+0xb8>
10016a810: aa1303e0    	mov	x0, x19
10016a814: aa1503e1    	mov	x1, x21
10016a818: 97feb6e7    	bl	0x1001183b4 <__RNCNvNtNtCs3JpjsLPrFbG_12nera_runtime3ffi10binary_ops22nera_rt_binary_to_list0B7_>
10016a81c: 14000014    	b	0x10016a86c <_nera_rt_binary_to_list+0x104>
10016a820: b0000900    	adrp	x0, 0x10028b000 <__RNvNCNKNvNtCs3JpjsLPrFbG_12nera_runtime9scheduler14CURRENT_WORKER0s_023___RUST_STD_INTERNAL_VAL+0x10>
10016a824: 91002000    	add	x0, x0, #0x8
10016a828: f9400008    	ldr	x8, [x0]
10016a82c: d63f0100    	blr	x8
10016a830: aa0003f4    	mov	x20, x0
10016a834: 3944a008    	ldrb	w8, [x0, #0x128]
10016a838: 7100051f    	cmp	w8, #0x1
10016a83c: 54000201    	b.ne	0x10016a87c <_nera_rt_binary_to_list+0x114>
10016a840: f9400288    	ldr	x8, [x20]
10016a844: b50002e8    	cbnz	x8, 0x10016a8a0 <_nera_rt_binary_to_list+0x138>
10016a848: 92800008    	mov	x8, #-0x1               ; =-1
10016a84c: aa1403e2    	mov	x2, x20
10016a850: f8008448    	str	x8, [x2], #0x8
10016a854: aa1303e0    	mov	x0, x19
10016a858: aa1503e1    	mov	x1, x21
10016a85c: 97feb6d6    	bl	0x1001183b4 <__RNCNvNtNtCs3JpjsLPrFbG_12nera_runtime3ffi10binary_ops22nera_rt_binary_to_list0B7_>
10016a860: f9400288    	ldr	x8, [x20]
10016a864: 91000508    	add	x8, x8, #0x1
10016a868: f9000288    	str	x8, [x20]
10016a86c: a9427bfd    	ldp	x29, x30, [sp, #0x20]
10016a870: a9414ff4    	ldp	x20, x19, [sp, #0x10]
10016a874: a8c357f6    	ldp	x22, x21, [sp], #0x30
10016a878: d65f03c0    	ret
10016a87c: aa1403e0    	mov	x0, x20
10016a880: d2800001    	mov	x1, #0x0                ; =0
10016a884: 940281da    	bl	0x10020afec <__RINvMs0_NtNtNtNtCs1SSuMAdGM58_3std3sys12thread_local6native4lazyINtB6_7StorageINtNtCsjIpoRzUjyHQ_4core4cell7RefCellNtNtCs3JpjsLPrFbG_12nera_runtime4heap11ProcessHeapEuE16get_or_init_slowNvNvNtNtB1T_3ffi7helpers12PROCESS_HEAP27___rust_std_internal_init_fnEB1T_>
10016a888: aa0003f4    	mov	x20, x0
10016a88c: b5fffda0    	cbnz	x0, 0x10016a840 <_nera_rt_binary_to_list+0xd8>
10016a890: d0000860    	adrp	x0, 0x100278000 <_writev+0x100278000>
10016a894: 91160000    	add	x0, x0, #0x580
10016a898: 9402f5d8    	bl	0x100227ff8 <__RNvNtNtCs1SSuMAdGM58_3std6thread5local18panic_access_error>
10016a89c: 14000004    	b	0x10016a8ac <_nera_rt_binary_to_list+0x144>
10016a8a0: d0000860    	adrp	x0, 0x100278000 <_writev+0x100278000>
10016a8a4: 9126c000    	add	x0, x0, #0x9b0
10016a8a8: 9402f7dc    	bl	0x100228818 <__RNvNtCsjIpoRzUjyHQ_4core4cell22panic_already_borrowed>
10016a8ac: d4200020    	brk	#0x1
10016a8b0: f9400288    	ldr	x8, [x20]
10016a8b4: 91000508    	add	x8, x8, #0x1
10016a8b8: f9000288    	str	x8, [x20]
10016a8bc: 9402f888    	bl	0x100228adc <__RNvNtCsjIpoRzUjyHQ_4core9panicking19panic_cannot_unwind>
10016a8c0: 9402f887    	bl	0x100228adc <__RNvNtCsjIpoRzUjyHQ_4core9panicking19panic_cannot_unwind>
