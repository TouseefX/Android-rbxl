/ fcn.140003df0(int64_t arg1, int64_t arg2, int64_t arg3, int64_t arg4);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; arg int64_t arg3 @ r8
|           ; arg int64_t arg4 @ r9
|           ; var int64_t var_148h @ stack - 0x148
|           ; var int64_t var_140h @ stack - 0x140
|           ; var int64_t var_138h @ stack - 0x138
|           ; var int64_t var_120h @ stack - 0x120
|           ; var int64_t var_118h @ stack - 0x118
|           ; var int64_t var_108h @ stack - 0x108
|           ; var int64_t var_f8h @ stack - 0xf8
|           ; var int64_t var_f0h @ stack - 0xf0
|           ; var int64_t var_e8h @ stack - 0xe8
|           ; var int64_t var_e0h @ stack - 0xe0
|           ; var int64_t var_d8h @ stack - 0xd8
|           ; var int64_t var_d0h @ stack - 0xd0
|           ; var int64_t var_c8h @ stack - 0xc8
|           ; var int64_t var_c0h @ stack - 0xc0
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_b0h @ stack - 0xb0
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_a0h @ stack - 0xa0
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_90h @ stack - 0x90
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_80h @ stack - 0x80
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_58h @ stack - 0x58
|           0x140003df0      push  rbp
|           0x140003df1      push  r15
|           0x140003df3      push  r14
|           0x140003df5      push  r13
|           0x140003df7      push  r12
|           0x140003df9      push  rsi
|           0x140003dfa      push  rdi
|           0x140003dfb      push  rbx
|           0x140003dfc      sub   rsp, 0x128
|           0x140003e03      lea   rbp, qword [var_e8h]
|           0x140003e0b      movaps xmmword [var_58h], xmm8
|           0x140003e13      movaps xmmword [var_68h], xmm7
|           0x140003e1a      movaps xmmword [var_78h], xmm6
|           0x140003e1e      mov   rdi, r9                             ; arg4
|           0x140003e21      mov   rsi, r8                             ; arg3
|           0x140003e24      mov   r14, rdx                            ; arg2
|           0x140003e27      mov   rbx, rcx                            ; arg1
|           0x140003e2a      call  0x140001b80
|           0x140003e2f      mov   rax, qword [rax]
|           0x140003e32      mov   qword [var_80h], rax
|           0x140003e36      mov   rax, qword [rax+0x20]
|           0x140003e3a      and   rax, 0xfffffffffffffffe
|       ,=< 0x140003e3e      jz    0x140003e61
|       |   0x140003e40      mov   qword [var_90h], 0x00
|       |   0x140003e48      mov   rax, qword [rax+0x28]
|       |   0x140003e4c      and   eax, 0x08
|       |   0x140003e4f      shr   eax, 0x03
|       |   0x140003e52      cmp   rdi, 0xffffffffffffffff
|       |   0x140003e56      setz  cl
|       |   0x140003e59      and   cl, al
|       |   0x140003e5b      mov   qword [var_98h], rcx
|      ,==< 0x140003e5f      jmp   0x140003e71
|      |`-> 0x140003e61      mov   qword [var_90h], 0x00
|      |    0x140003e69      mov   qword [var_98h], 0x00
|      |    ; CODE XREF from fcn.140003df0 @ 0x140003e5f
|      `--> 0x140003e71      lea   rax, qword [r14+0x10]
|           0x140003e75      mov   qword [var_f8h], rax
|           0x140003e79      lea   rax, qword [rbx+0x7c0]
|           0x140003e80      mov   qword [rbp], rax
|           0x140003e84      lea   rax, qword [rbx+0x680]
|           0x140003e8b      mov   qword [var_f0h], rax
|           0x140003e8f      lea   r12, qword [var_e0h]
|           0x140003e93      movsd xmm6, qword [0x1484ae818]           ; [0x1484ae818:8]=0x43e0000000000000
|           0x140003e9b      movsd xmm7, qword [0x1484ae810]           ; [0x1484ae810:8]=0x412e848000000000
|           0x140003ea3      xorps xmm8, xmm8
|           0x140003ea7      mov   qword [var_88h], 0x00
|       ,=< 0x140003eaf      jmp   0x140003ed4
..
|       |   ; CODE XREF from fcn.140003df0 @ 0x14000432f
|     ..--> 0x140003ec0      xor   r13d, r13d
|  ...----> 0x140003ec3      mov   eax, r13d
|  :::::|   0x140003ec6      mov   qword [var_88h], rax
|  :::::|   0x140003eca      test  r13b, 0x01                          ; 1
| ,=======< 0x140003ece      jnz   0x1400044c7
| |:::::|   ; CODE XREF from fcn.140003df0 @ 0x140003eaf
| |:::::`-> 0x140003ed4      mov   rcx, qword [rsi+0x08]
| |:::::    0x140003ed8      xor   edx, edx
| |:::::    0x140003eda      call  qword [rsi]
| |:::::    0x140003edc      test  rax, rax
| |:::::,=< 0x140003edf      jnz   0x1400044c1
| |:::::|   0x140003ee5      mov   rax, qword [var_80h]
| |:::::|   0x140003ee9      mov   eax, dword [rax+0x20]
| |:::::|   0x140003eec      and   eax, 0x01
| |:::::|   0x140003eef      lea   rcx, qword [r14+rax*8]
| |:::::|   0x140003ef3      mov   qword [var_b0h], rcx
| |:::::|   0x140003ef7      mov   rax, qword [r14+rax*8]
| |:::::|   0x140003efb      shr   rax, 0x30
| |:::::|   0x140003eff      mov   qword [var_a8h], rax
| |:::::|   0x140003f03      mov   rcx, qword [rsi+0x08]
| |:::::|   0x140003f07      xor   edx, edx
| |:::::|   0x140003f09      call  qword [rsi]
| |:::::|   0x140003f0b      test  rax, rax
| ========< 0x140003f0e      jnz   0x1400044c1
| |:::::|   0x140003f14      mov   rax, qword [rbx+0x1b090]
| |:::::|   0x140003f1b      mov   eax, dword [rax]
| |:::::|   0x140003f1d      test  eax, eax
| ========< 0x140003f1f      js    0x140004070
| |:::::|   0x140003f25      mov   rcx, qword [var_90h]
| |:::::|   0x140003f29      inc   ecx
| |:::::|   0x140003f2b      mov   rax, qword [rbx+0x1b090]
| |:::::|   0x140003f32      mov   eax, dword [rax]
| |:::::|   0x140003f34      mov   qword [var_90h], rcx
| |:::::|   0x140003f38      cmp   ecx, eax
| ========< 0x140003f3a      jnz   0x140004070
| |:::::|   0x140003f40      mov   rax, qword [rbx+0x1b098]
| |:::::|   0x140003f47      mov   eax, dword [rax]
| |:::::|   0x140003f49      mov   qword [var_90h], 0x00
| |:::::|   0x140003f51      test  eax, eax
| ========< 0x140003f53      js    0x140004070
| |:::::|   0x140003f59      call  0x140001b80
| |:::::|   0x140003f5e      mov   rax, qword [rax]
| |:::::|   0x140003f61      mov   rax, qword [rax+0x68]
| |:::::|   0x140003f65      mov   rax, qword [rax+0x60]
| |:::::|   0x140003f69      test  rax, rax
| ========< 0x140003f6c      jz    0x140003f85
| |:::::|   0x140003f6e      mov   ecx, dword [rax+0x60]
| |:::::|   0x140003f71      test  cl, 0x02                            ; 2
| |:::::|   0x140003f74      mov   r15, qword [sym.imp.KERNEL32.dll_QueryPerformanceCounter] ; [0x148438b00:8]=0xc35af62 ; "b\xaf5\f"
| ========< 0x140003f7b      jz    0x140003fa0
| |:::::|   0x140003f7d      add   rax, 0xa0                           ; 160
| ========< 0x140003f83      jmp   0x140003f9b
| --------> 0x140003f85      mov   ecx, dword [rbx+0x780]
| |:::::|   0x140003f8b      mov   rax, qword [rbp]
| |:::::|   0x140003f8f      test  cl, 0x02                            ; 2
| |:::::|   0x140003f92      mov   r15, qword [sym.imp.KERNEL32.dll_QueryPerformanceCounter] ; [0x148438b00:8]=0xc35af62 ; "b\xaf5\f"
| ========< 0x140003f99      jz    0x140003fa0
| |:::::|   ; CODE XREF from fcn.140003df0 @ 0x140003f83
| --------> 0x140003f9b      lock  inc qword [rax+0x18]
| --------> 0x140003fa0      mov   rax, qword [rbx+0x1b098]
| |:::::|   0x140003fa7      mov   eax, dword [rax]
| |:::::|   0x140003fa9      test  eax, eax
| ========< 0x140003fab      jz    0x140004067
| |:::::|   0x140003fb1      mov   rax, qword [var_80h]
| |:::::|   0x140003fb5      mov   r13, qword [rax+0x68]
| |:::::|   0x140003fb9      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| |:::::|   0x140003fbf      cmp   eax, 0x02                           ; 2
| ========< 0x140003fc2      jz    0x140004002
| |:::::|   0x140003fc4      xor   eax, eax
| |:::::|   0x140003fc6      mov   ecx, 0x01
| |:::::|   0x140003fcb      lock  cmpxchg dword [0x14cb874a8], ecx
| ========< 0x140003fd3      jnz   0x140004120
| |:::::|   0x140003fd9      mov   rcx, r12
| |:::::|   0x140003fdc      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceFrequency] ; [0x148438af8:8]=0xc35af46 ; "F\xaf5\f" ; BOOL QueryPerformanceFrequency(LARGE_INTEGER *lpFrequency)
| |:::::|   0x140003fe2      cvtsi2sd xmm0, qword [var_e0h]
| |:::::|   0x140003fe8      movapd xmm1, xmm7
| |:::::|   0x140003fec      divsd xmm1, xmm0
| |:::::|   0x140003ff0      movsd qword [0x14cb874b0], xmm1           ; [0x14cb874b0:8]=0
| |:::::|   0x140003ff8      mov   dword [0x14cb874a8], 0x02           ; [0x14cb874a8:4]=0
| --------> 0x140004002      mov   rcx, r12
| |:::::|   0x140004005      call  r15
| |:::::|   0x140004008      xorps xmm0, xmm0
| |:::::|   0x14000400b      cvtsi2sd xmm0, qword [var_e0h]
| |:::::|   0x140004011      mulsd xmm0, qword [0x14cb874b0]
| |:::::|   0x140004019      cvttsd2si rax, xmm0
| |:::::|   0x14000401e      mov   rcx, rax
| |:::::|   0x140004021      sar   rcx, 0x3f
| |:::::|   0x140004025      subsd xmm0, xmm6
| |:::::|   0x140004029      cvttsd2si rdx, xmm0
| |:::::|   0x14000402e      and   rdx, rcx
| |:::::|   0x140004031      or    rdx, rax
| |:::::|   0x140004034      mov   rax, qword [rbx+0x1b098]
| |:::::|   0x14000403b      mov   eax, dword [rax]
| |:::::|   0x14000403d      movsxd r9, eax
| |:::::|   0x140004040      add   r9, rdx
| |:::::|   0x140004043      mov   rax, qword [var_80h]
| |:::::|   0x140004047      mov   rax, qword [rax+0x68]
| |:::::|   0x14000404b      mov   r8, qword [rax+0x08]
| |:::::|   0x14000404f      shr   r8, 0x20
| |:::::|   0x140004053      mov   rax, qword [var_80h]
| |:::::|   0x140004057      mov   rdx, qword [rax+0x68]
| |:::::|   0x14000405b      mov   rcx, r13
| |:::::|   0x14000405e      call  0x140001410
| |:::::|   0x140004063      test  al, al
| ========< 0x140004065      jnz   0x140004070
| --------> 0x140004067      pause
| |:::::|   0x140004069      nop   dword [rax], eax
| --------> 0x140004070      mov   r15, r14
| |:::::|   0x140004073      mov   r14, rsi
| |:::::|   0x140004076      mov   qword [var_a0h], rdi
| |:::::|   0x14000407a      mov   rsi, rdi
| |:::::|   0x14000407d      cmp   byte [var_98h], 0x00
| ========< 0x140004081      jz    0x140004154
| |:::::|   0x140004087      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| |:::::|   0x14000408d      cmp   eax, 0x02                           ; 2
| ========< 0x140004090      jz    0x1400040cf
| |:::::|   0x140004092      xor   eax, eax
| |:::::|   0x140004094      mov   ecx, 0x01
| |:::::|   0x140004099      lock  cmpxchg dword [0x14cb874a8], ecx
| ========< 0x1400040a1      jnz   0x140004110
| |:::::|   0x1400040a3      mov   rcx, r12
| |:::::|   0x1400040a6      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceFrequency] ; [0x148438af8:8]=0xc35af46 ; "F\xaf5\f" ; BOOL QueryPerformanceFrequency(LARGE_INTEGER *lpFrequency)
| |:::::|   0x1400040ac      xorps xmm0, xmm0
| |:::::|   0x1400040af      cvtsi2sd xmm0, qword [var_e0h]
| |:::::|   0x1400040b5      movapd xmm1, xmm7
| |:::::|   0x1400040b9      divsd xmm1, xmm0
| |:::::|   0x1400040bd      movsd qword [0x14cb874b0], xmm1           ; [0x14cb874b0:8]=0
| |:::::|   0x1400040c5      mov   dword [0x14cb874a8], 0x02           ; [0x14cb874a8:4]=0
| --------> 0x1400040cf      mov   rcx, r12
| |:::::|   0x1400040d2      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceCounter] ; [0x148438b00:8]=0xc35af62 ; "b\xaf5\f" ; BOOL QueryPerformanceCounter(LARGE_INTEGER *lpPerformanceCount)
| |:::::|   0x1400040d8      xorps xmm0, xmm0
| |:::::|   0x1400040db      cvtsi2sd xmm0, qword [var_e0h]
| |:::::|   0x1400040e1      mulsd xmm0, qword [0x14cb874b0]
| |:::::|   0x1400040e9      movapd xmm1, xmm0
| |:::::|   0x1400040ed      subsd xmm1, xmm6
| |:::::|   0x1400040f1      mov   rax, qword [rbx+0x1b0a8]
| |:::::|   0x1400040f8      mov   eax, dword [rax]
| |:::::|   0x1400040fa      test  eax, eax
| ========< 0x1400040fc      jle   0x140004131
| |:::::|   0x1400040fe      mov   rax, qword [rbx+0x1b0a8]
| |:::::|   0x140004105      mov   eax, dword [rax]
| |:::::|   0x140004107      cdqe
| ========< 0x140004109      jmp   0x140004136
..
| |:::::|   ; CODE XREF from fcn.140003df0 @ 0x14000411b
| --------> 0x140004110      cmp   eax, 0x02                           ; 2
| ========< 0x140004113      jz    0x1400040cf
| |:::::|   0x140004115      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| ========< 0x14000411b      jmp   0x140004110
..
| |:::::|   ; CODE XREF from fcn.140003df0 @ 0x14000412f
| --------> 0x140004120      cmp   eax, 0x02                           ; 2
| ========< 0x140004123      jz    0x140004002
| |:::::|   0x140004129      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| ========< 0x14000412f      jmp   0x140004120
| --------> 0x140004131      mov   eax, 0x989680
| |:::::|   ; CODE XREF from fcn.140003df0 @ 0x140004109
| --------> 0x140004136      cvttsd2si rcx, xmm0
| |:::::|   0x14000413b      mov   rdx, rcx
| |:::::|   0x14000413e      sar   rdx, 0x3f
| |:::::|   0x140004142      cvttsd2si rsi, xmm1
| |:::::|   0x140004147      and   rsi, rdx
| |:::::|   0x14000414a      or    rsi, rcx
| |:::::|   0x14000414d      add   rsi, rax
| |:::::|   0x140004150      mov   qword [var_a0h], rsi
| --------> 0x140004154      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| |:::::|   0x14000415a      cmp   eax, 0x02                           ; 2
| ========< 0x14000415d      jz    0x1400041a0
| |:::::|   0x14000415f      xor   eax, eax
| |:::::|   0x140004161      mov   ecx, 0x01
| |:::::|   0x140004166      lock  cmpxchg dword [0x14cb874a8], ecx
| ========< 0x14000416e      jnz   0x1400042b0
| |:::::|   0x140004174      mov   rcx, r12
| |:::::|   0x140004177      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceFrequency] ; [0x148438af8:8]=0xc35af46 ; "F\xaf5\f" ; BOOL QueryPerformanceFrequency(LARGE_INTEGER *lpFrequency)
| |:::::|   0x14000417d      xorps xmm0, xmm0
| |:::::|   0x140004180      cvtsi2sd xmm0, qword [var_e0h]
| |:::::|   0x140004186      movapd xmm1, xmm7
| |:::::|   0x14000418a      divsd xmm1, xmm0
| |:::::|   0x14000418e      movsd qword [0x14cb874b0], xmm1           ; [0x14cb874b0:8]=0
| |:::::|   0x140004196      mov   dword [0x14cb874a8], 0x02           ; [0x14cb874a8:4]=0
| --------> 0x1400041a0      mov   rcx, r12
| |:::::|   0x1400041a3      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceCounter] ; [0x148438b00:8]=0xc35af62 ; "b\xaf5\f" ; BOOL QueryPerformanceCounter(LARGE_INTEGER *lpPerformanceCount)
| |:::::|   0x1400041a9      xorps xmm0, xmm0
| |:::::|   0x1400041ac      cvtsi2sd xmm0, qword [var_e0h]
| |:::::|   0x1400041b2      mulsd xmm0, qword [0x14cb874b0]
| |:::::|   0x1400041ba      cvttsd2si rax, xmm0
| |:::::|   0x1400041bf      mov   rcx, rax
| |:::::|   0x1400041c2      sar   rcx, 0x3f
| |:::::|   0x1400041c6      subsd xmm0, xmm6
| |:::::|   0x1400041ca      cvttsd2si rdx, xmm0
| |:::::|   0x1400041cf      and   rdx, rcx
| |:::::|   0x1400041d2      or    rdx, rax
| |:::::|   0x1400041d5      mov   r13b, 0x01
| |:::::|   0x1400041d8      cmp   rsi, rdx
| |:::::|   0x1400041db      mov   rsi, r14
| |:::::|   0x1400041de      mov   r14, r15
| ========< 0x1400041e1      jbe   0x140004307
| |:::::|   0x1400041e7      movaps xmmword [var_108h], xmm8
| |:::::|   0x1400041ec      movaps xmmword [var_118h], xmm8
| |:::::|   0x1400041f1      lea   rax, qword [var_a0h]
| |:::::|   0x1400041f5      mov   qword [var_e0h], rax
| |:::::|   0x1400041f9      mov   qword [var_d8h], rbx
| |:::::|   0x1400041fd      lea   rax, qword [var_118h]
| |:::::|   0x140004201      mov   qword [var_d0h], rax
| |:::::|   0x140004205      mov   qword [var_c8h], r14
| |:::::|   0x140004209      lea   rax, qword [var_b0h]
| |:::::|   0x14000420d      mov   qword [var_c0h], rax
| |:::::|   0x140004211      lea   rax, qword [var_80h]
| |:::::|   0x140004215      mov   qword [var_b8h], rax
| |:::::|   0x140004219      mov   rcx, qword [var_80h]
| |:::::|   0x14000421d      mov   rdx, qword [rcx+0x70]
| |:::::|   0x140004221      mov   rax, qword [var_b0h]
| |:::::|   0x140004225      mov   rax, qword [rax]
| |:::::|   0x140004228      shr   rax, 0x30
| |:::::|   0x14000422c      cmp   qword [var_a8h], rax
| ========< 0x140004230      jnz   0x140004269
| |:::::|   0x140004232      and   rdx, 0xffffffffffffffc0
| |:::::|   0x140004236      mov   qword [var_138h], rdx
| |:::::|   0x14000423a      movupd xmm0, xmmword [var_b0h]
| |:::::|   0x14000423f      movupd xmmword [var_138h + 0x8], xmm0
| |:::::|   0x140004244      mov   qword [var_120h], r12
| |:::::|   0x140004248      mov   rax, qword [var_f8h]
| |:::::|   0x14000424c      mov   qword [var_140h], rax
| |:::::|   0x140004251      mov   dword [var_148h], 0x01
| |:::::|   0x140004259      lea   r8, qword [var_138h]
| |:::::|   0x14000425d      xor   r9d, r9d
| |:::::|   0x140004260      call  0x140011650
| |:::::|   0x140004265      test  al, al
| ========< 0x140004267      jz    0x1400042c1
| --------> 0x140004269      cmp   qword [var_118h], 0x00
| ========< 0x14000426e      jz    0x140004300
| |:::::|   0x140004274      mov   rcx, qword [var_f0h]
| |:::::|   0x140004278      lea   rdx, qword [var_118h]
| |:::::|   0x14000427c      call  0x140010380
| |:::::|   0x140004281      mov   r13b, 0x01
| |:::::|   0x140004284      test  al, al
| ========< 0x140004286      jz    0x140004307
| |:::::|   0x140004288      mov   ax, 0xffff
| |:::::|   0x14000428c      lock  xadd word [r14+0x26], ax
| |:::::|   0x140004293      cmp   ax, 0x01                            ; 1
| ========< 0x140004297      jnz   0x1400042d7
| |:::::|   0x140004299      movzx ecx, byte [r14+0x25]
| |:::::|   0x14000429e      xor   edx, edx
| |:::::|   0x1400042a0      mov   r8, r14
| |:::::|   0x1400042a3      call  0x1400036b0
| ========< 0x1400042a8      jmp   0x140004300
..
| |:::::|   ; CODE XREF from fcn.140003df0 @ 0x1400042bf
| --------> 0x1400042b0      cmp   eax, 0x02                           ; 2
| ========< 0x1400042b3      jz    0x1400041a0
| |:::::|   0x1400042b9      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| ========< 0x1400042bf      jmp   0x1400042b0
| --------> 0x1400042c1      mov   r8, qword [var_a0h]
| |:::::|   0x1400042c5      mov   rdx, qword [var_80h]
| |:::::|   0x1400042c9      lea   rcx, qword [var_b0h]
| |:::::|   0x1400042cd      call  0x140010560
| |:::::|   0x1400042d2      mov   r13d, eax
| ========< 0x1400042d5      jmp   0x140004307
| --------> 0x1400042d7      test  ax, ax
| |:::::|   0x1400042da      setz  cl
| |:::::|   0x1400042dd      mov   rax, qword [0x14c39fe98]            ; [0x14c39fe98:8]=0x1428bd120 ; " ыB\U00000001"
| |:::::|   0x1400042e4      test  rax, rax
| |:::::|   0x1400042e7      setnz dl
| |:::::|   0x1400042ea      and   dl, cl
| |:::::|   0x1400042ec      cmp   dl, 0x01                            ; 1
| ========< 0x1400042ef      jnz   0x140004300
| |:::::|   0x1400042f1      lea   rcx, qword [0x1484aea05]            ; "257f718-Object,55"
| |:::::|   0x1400042f8      call  rax
| |:::::|   0x1400042fa      nop   word [rax+rax*1], ax
| |:::::|   ; CODE XREF from fcn.140003df0 @ 0x1400042a8
| --------> 0x140004300      mov   rax, qword [var_88h]
| |:::::|   0x140004304      mov   r13d, eax
| |:::::|   ; CODE XREF from fcn.140003df0 @ 0x1400042d5
| --------> 0x140004307      mov   eax, r13d
| |:::::|   0x14000430a      not   al
| |:::::|   0x14000430c      or    al, byte [var_98h]
| |:::::|   0x14000430f      test  al, 0x01                            ; 1
| ========< 0x140004311      jz    0x140004340
| |:::::|   0x140004313      mov   rax, qword [0x14cb874c8]            ; [0x14cb874c8:8]=0
| |:::::|   0x14000431a      test  rax, rax
| |:::::|   0x14000431d      setnz cl
| |:::::|   0x140004320      and   r13b, cl
| |:::::|   0x140004323      cmp   r13b, 0x01                          ; 1
| |:::`===< 0x140004327      jnz   0x140003ec0
| |::: :|   0x14000432d      call  rax
| |::: `==< 0x14000432f      jmp   0x140003ec0
..
| --------> 0x140004340      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| |:::  |   0x140004346      cmp   eax, 0x02                           ; 2
| |:::  |   0x140004349      mov   r15, qword [sym.imp.KERNEL32.dll_QueryPerformanceCounter] ; [0x148438b00:8]=0xc35af62 ; "b\xaf5\f"
| |::: ,==< 0x140004350      jz    0x140004393
| |::: ||   0x140004352      xor   eax, eax
| |::: ||   0x140004354      mov   ecx, 0x01
| |::: ||   0x140004359      lock  cmpxchg dword [0x14cb874a8], ecx
| |:::,===< 0x140004361      jnz   0x1400044b0
| |:::|||   0x140004367      mov   rcx, r12
| |:::|||   0x14000436a      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceFrequency] ; [0x148438af8:8]=0xc35af46 ; "F\xaf5\f" ; BOOL QueryPerformanceFrequency(LARGE_INTEGER *lpFrequency)
| |:::|||   0x140004370      xorps xmm0, xmm0
| |:::|||   0x140004373      cvtsi2sd xmm0, qword [var_e0h]
| |:::|||   0x140004379      movapd xmm1, xmm7
| |:::|||   0x14000437d      divsd xmm1, xmm0
| |:::|||   0x140004381      movsd qword [0x14cb874b0], xmm1           ; [0x14cb874b0:8]=0
| |:::|||   0x140004389      mov   dword [0x14cb874a8], 0x02           ; [0x14cb874a8:4]=0
| -----`--> 0x140004393      mov   rcx, r12
| |:::| |   0x140004396      call  r15
| |:::| |   0x140004399      xorps xmm0, xmm0
| |:::| |   0x14000439c      cvtsi2sd xmm0, qword [var_e0h]
| |:::| |   0x1400043a2      mulsd xmm0, qword [0x14cb874b0]
| |:::| |   0x1400043aa      cvttsd2si rax, xmm0
| |:::| |   0x1400043af      mov   rcx, rax
| |:::| |   0x1400043b2      subsd xmm0, xmm6
| |:::| |   0x1400043b6      cvttsd2si rdx, xmm0
| |:::| |   0x1400043bb      sar   rcx, 0x3f
| |:::| |   0x1400043bf      and   rdx, rcx
| |:::| |   0x1400043c2      or    rdx, rax
| |:::| |   0x1400043c5      mov   rcx, rdi
| |:::| |   0x1400043c8      sub   rcx, rdx
| |:::| |   0x1400043cb      mov   eax, 0x00
| |:::| |   0x1400043d0      cmovnb rax, rcx
| |:::| |   0x1400043d4      mov   rcx, qword [rbx+0x1b0a0]
| |:::| |   0x1400043db      mov   ecx, dword [rcx]
| |:::| |   0x1400043dd      test  rax, rax
| |`======< 0x1400043e0      jle   0x140003ec3
| | ::| |   0x1400043e6      movsxd rcx, ecx
| | ::| |   0x1400043e9      cmp   rax, rcx
| | `=====< 0x1400043ec      jnle  0x140003ec3
| |  :| |   0x1400043f2      nop   word [rax+rax*1], ax
| |  :| |   0x140004400      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| |  :| |   0x140004406      cmp   eax, 0x02                           ; 2
| |  :|,==< 0x140004409      jz    0x14000444c
| | .-----> 0x14000440b      xor   eax, eax
| | ::|||   0x14000440d      mov   ecx, 0x01
| | ::|||   0x140004412      lock  cmpxchg dword [0x14cb874a8], ecx
| |,======< 0x14000441a      jnz   0x1400044a0
| ||::|||   0x140004420      mov   rcx, r12
| ||::|||   0x140004423      call  qword [sym.imp.KERNEL32.dll_QueryPerformanceFrequency] ; [0x148438af8:8]=0xc35af46 ; "F\xaf5\f" ; BOOL QueryPerformanceFrequency(LARGE_INTEGER *lpFrequency)
| ||::|||   0x140004429      xorps xmm0, xmm0
| ||::|||   0x14000442c      cvtsi2sd xmm0, qword [var_e0h]
| ||::|||   0x140004432      movapd xmm1, xmm7
| ||::|||   0x140004436      divsd xmm1, xmm0
| ||::|||   0x14000443a      movsd qword [0x14cb874b0], xmm1           ; [0x14cb874b0:8]=0
| ||::|||   0x140004442      mov   dword [0x14cb874a8], 0x02           ; [0x14cb874a8:4]=0
| ||::|||   ; CODE XREF from fcn.140003df0 @ 0x140004498
| -----`--> 0x14000444c      mov   rcx, r12
| ||::| |   0x14000444f      call  r15
| ||::| |   0x140004452      xorps xmm0, xmm0
| ||::| |   0x140004455      cvtsi2sd xmm0, qword [var_e0h]
| ||::| |   0x14000445b      mulsd xmm0, qword [0x14cb874b0]
| ||::| |   0x140004463      cvttsd2si rax, xmm0
| ||::| |   0x140004468      mov   rcx, rax
| ||::| |   0x14000446b      subsd xmm0, xmm6
| ||::| |   0x14000446f      cvttsd2si rdx, xmm0
| ||::| |   0x140004474      sar   rcx, 0x3f
| ||::| |   0x140004478      and   rdx, rcx
| ||::| |   0x14000447b      or    rdx, rax
| ||::| |   0x14000447e      cmp   rdi, rdx
| ||:`====< 0x140004481      jbe   0x140003ec3
| ||: | |   0x140004487      pause
| ||: | |   0x140004489      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| ||: | |   0x14000448f      cmp   eax, 0x02                           ; 2
| ||`=====< 0x140004492      jnz   0x14000440b
| ========< 0x140004498      jmp   0x14000444c
..
| ||  | |   ; CODE XREF from fcn.140003df0 @ 0x1400044ab
| |`---.--> 0x1400044a0      cmp   eax, 0x02                           ; 2
| ========< 0x1400044a3      jz    0x14000444c
| |   |:|   0x1400044a5      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| |   |`==< 0x1400044ab      jmp   0x1400044a0
..
| |   | |   ; CODE XREF from fcn.140003df0 @ 0x1400044bf
| |   `.--> 0x1400044b0      cmp   eax, 0x02                           ; 2
| ========< 0x1400044b3      jz    0x140004393
| |    :|   0x1400044b9      mov   eax, dword [0x14cb874a8]            ; [0x14cb874a8:4]=0
| |    `==< 0x1400044bf      jmp   0x1400044b0
| ------`-> 0x1400044c1      mov   rax, qword [var_88h]
| |     ,=< 0x1400044c5      jmp   0x1400044c9
| `-------> 0x1400044c7      mov   al, 0x01
|       |   ; CODE XREF from fcn.140003df0 @ 0x1400044c5
|       `-> 0x1400044c9      and   al, 0x01
|           0x1400044cb      movaps xmm6, xmmword [var_78h]
|           0x1400044cf      movaps xmm7, xmmword [var_68h]
|           0x1400044d6      movaps xmm8, xmmword [var_58h]
|           0x1400044de      add   rsp, 0x128
|           0x1400044e5      pop   rbx
|           0x1400044e6      pop   rdi
|           0x1400044e7      pop   rsi
|           0x1400044e8      pop   r12
|           0x1400044ea      pop   r13
|           0x1400044ec      pop   r14
|           0x1400044ee      pop   r15
|           0x1400044f0      pop   rbp
\           0x1400044f1      ret
