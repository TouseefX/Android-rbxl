/ fcn.143447550(int64_t arg1);
|       :   ; arg int64_t arg1 @ rcx
|       :   ; var int64_t var_8h @ stack + 0x8
|       :   ; var int64_t var_10h @ stack + 0x10
|       :   ; var int64_t var_18h @ stack + 0x18
|       :   ; var int64_t var_20h @ stack + 0x20
|       :   0x143447550      mov   qword [var_8h], rbx
|       :   0x143447555      mov   qword [var_10h], rbp
|       :   0x14344755a      mov   qword [var_18h], rsi
|       :   0x14344755f      mov   qword [var_20h], rdi
|       :   0x143447564      push  r14
|       :   0x143447566      sub   rsp, 0x30
|       :   0x14344756a      mov   rsi, rcx                            ; arg1
|       :   0x14344756d      mov   rdi, qword [rcx+0x40]               ; arg1
|       :   0x143447571      mov   rbx, qword [rcx+0x38]               ; arg1
|       :   0x143447575      cmp   rbx, rdi
|      ,==< 0x143447578      jz    0x143447593
|      |:   0x14344757a      nop   word [rax+rax*1], ax
|     .---> 0x143447580      xor   edx, edx
|     :|:   0x143447582      mov   rcx, qword [rbx]
|     :|:   0x143447585      call  0x143446160
|     :|:   0x14344758a      add   rbx, 0x08
|     :|:   0x14344758e      cmp   rbx, rdi
|     `===< 0x143447591      jnz   0x143447580
|      `--> 0x143447593      mov   rbx, qword [rsi+0x60]
|       :   0x143447597      mov   rdi, qword [rsi+0x68]
|       :   0x14344759b      cmp   rbx, rdi
|      ,==< 0x14344759e      jz    0x1434475e3
|     .---> 0x1434475a0      cmp   dword [rbx+0x08], 0x00
|    ,====< 0x1434475a4      jz    0x14344764e
|    |:|:   0x1434475aa      call  0x1473107b8
|    |:|:   0x1434475af      cmp   dword [rbx+0x08], eax
|   ,=====< 0x1434475b2      jz    0x143447643
|   ||:|:   0x1434475b8      movups xmm0, xmmword [rbx]
|   ||:|:   0x1434475bb      movaps xmmword [rsp+0x20], xmm0
|   ||:|:   0x1434475c0      xor   edx, edx
|   ||:|:   0x1434475c2      lea   rcx, qword [rsp+0x20]
|   ||:|:   0x1434475c7      call  0x1473107b2
|   ||:|:   0x1434475cc      test  eax, eax
|  ,======< 0x1434475ce      jnz   0x143447659
|  |||:|:   0x1434475d4      xorps xmm0, xmm0
|  |||:|:   0x1434475d7      movups xmmword [rbx], xmm0
|  |||:|:   0x1434475da      add   rbx, 0x10                           ; 16
|  |||:|:   0x1434475de      cmp   rbx, rdi
|  |||`===< 0x1434475e1      jnz   0x1434475a0
|  ||| `--> 0x1434475e3      lea   rcx, qword [rsi+0x58]
|  |||  :   0x1434475e7      call  0x1434474e0
|  |||  :   0x1434475ec      mov   rbx, qword [rsi+0x50]
|  |||  :   0x1434475f0      test  rbx, rbx
|  ||| ,==< 0x1434475f3      jz    0x143447605
|  ||| |:   0x1434475f5      mov   rcx, rbx
|  ||| |:   0x1434475f8      call  0x14344c230
|  ||| |:   0x1434475fd      mov   rcx, rbx
|  ||| |:   0x143447600      call  fcn.14385bd60
|  ||| `--> 0x143447605      lea   rcx, qword [rsi+0x30]
|  |||  :   0x143447609      call  0x143447440
|  |||  :   0x14344760e      mov   rcx, qword [rsi+0x28]
|  |||  :   0x143447612      test  rcx, rcx
|  ||| ,==< 0x143447615      jz    0x143447621
|  ||| |:   0x143447617      mov   rax, qword [rcx]
|  ||| |:   0x14344761a      mov   edx, 0x01
|  ||| |:   0x14344761f      call  qword [rax]
|  ||| `--> 0x143447621      mov   rcx, rsi
|  |||  :   0x143447624      mov   rbx, qword [var_8h]
|  |||  :   0x143447629      mov   rbp, qword [var_10h]
|  |||  :   0x14344762e      mov   rsi, qword [var_18h]
|  |||  :   0x143447633      mov   rdi, qword [var_20h]
|  |||  :   0x143447638      add   rsp, 0x30
|  |||  :   0x14344763c      pop   r14
|  |||  `=< 0x14344763e      jmp   0x14073a8e0
|  |`-----> 0x143447643      mov   ecx, 0x05
|  | |      0x143447648      call  0x1473107c4
|  | |      0x14344764d      int3
|  | `----> 0x14344764e      invalid
|  |        0x14344764f      invalid
|  |        0x143447650      add   byte [rax], al
|  |        0x143447652      add   al, ch
|  |        0x143447654      insb
|  |        0x143447655      xchg  ecx, eax
|  |        0x143447656      in    al, dx
|  |        0x143447657      add   ecx, esp
|  `------> 0x143447659      mov   ecx, 0x02
|           0x14344765e      call  0x1473107c4
|           0x143447663      int3
            0x143447664      int3
            0x143447665      int3
