/ fcn.14603e4e0(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_118h @ stack - 0x118
|           ; var int64_t var_108h @ stack - 0x108
|           ; var int64_t var_100h @ stack - 0x100
|           ; var int64_t var_f8h @ stack - 0xf8
|           ; var int64_t var_f0h @ stack - 0xf0
|           ; var int64_t var_e8h @ stack - 0xe8
|           ; var int64_t var_e0h @ stack - 0xe0
|           ; var int64_t var_d8h @ stack - 0xd8
|           ; var int64_t var_c8h @ stack - 0xc8
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_a0h @ stack - 0xa0
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_90h @ stack - 0x90
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           ; var int64_t var_20h @ stack + 0x20
|           0x14603e4e0      mov   qword [var_10h], rbx
|           0x14603e4e5      mov   qword [var_18h], rsi
|           0x14603e4ea      mov   qword [var_20h], rdi
|           0x14603e4ef      push  rbp
|           0x14603e4f0      push  r12
|           0x14603e4f2      push  r13
|           0x14603e4f4      push  r14
|           0x14603e4f6      push  r15
|           0x14603e4f8      lea   rbp, qword [var_38h]
|           0x14603e4fd      sub   rsp, 0x110
|           0x14603e504      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14603e50b      xor   rax, rsp
|           0x14603e50e      mov   qword [rbp], rax
|           0x14603e512      mov   rsi, rdx                            ; arg2
|           0x14603e515      mov   qword [var_f8h], rcx                ; arg1
|           0x14603e51a      lea   r13, qword [rdx+0x10]               ; arg2
|           0x14603e51e      cmp   qword [rdx+0x18], 0x10              ; arg2
|       ,=< 0x14603e523      jb    0x14603e531
|       |   0x14603e525      mov   rbx, qword [rdx]                    ; arg2
|       |   0x14603e528      add   rbx, qword [r13]
|       |   0x14603e52c      mov   rax, qword [rdx]                    ; arg2
|      ,==< 0x14603e52f      jmp   0x14603e53b
|      |`-> 0x14603e531      mov   rbx, qword [r13]
|      |    0x14603e535      add   rbx, rsi
|      |    0x14603e538      mov   rax, rsi
|      |    ; CODE XREF from fcn.14603e4e0 @ 0x14603e52f
|      `--> 0x14603e53b      mov   qword [var_108h], rax
|           0x14603e540      xorps xmm0, xmm0
|           0x14603e543      movdqu xmmword [var_e8h], xmm0
|           0x14603e549      xor   r14d, r14d
|           0x14603e54c      mov   qword [var_d8h], r14
|           0x14603e551      xor   r15d, r15d
|           0x14603e554      xor   r12d, r12d
|           0x14603e557      sub   rbx, rax
|       ,=< 0x14603e55a      jz    0x14603e5d7
|       |   0x14603e55c      mov   rax, 0x7fffffffffffffff             ; 9223372036854775807
|       |   0x14603e566      cmp   rbx, rax
|      ,==< 0x14603e569      jnbe  0x14603e79f
|      ||   0x14603e56f      cmp   rbx, 0x1000
|     ,===< 0x14603e576      jb    0x14603e5a1
|     |||   0x14603e578      lea   rcx, qword [rbx+0x27]
|     |||   0x14603e57c      cmp   rcx, rbx
|    ,====< 0x14603e57f      jbe   0x14603e799
|    ||||   0x14603e585      call  0x14385bce0
|    ||||   0x14603e58a      test  rax, rax
|   ,=====< 0x14603e58d      jz    0x14603e75a
|   |||||   0x14603e593      lea   r15, qword [rax+0x27]
|   |||||   0x14603e597      and   r15, 0xffffffffffffffe0
|   |||||   0x14603e59b      mov   qword [r15-0x08], rax
|  ,======< 0x14603e59f      jmp   0x14603e5ac
|  |||`---> 0x14603e5a1      mov   rcx, rbx
|  ||| ||   0x14603e5a4      call  0x14385bce0
|  ||| ||   0x14603e5a9      mov   r15, rax
|  ||| ||   ; CODE XREF from fcn.14603e4e0 @ 0x14603e59f
|  `------> 0x14603e5ac      mov   rdi, r15
|   || ||   0x14603e5af      mov   qword [var_e8h], r15
|   || ||   0x14603e5b4      lea   r14, qword [r15+rbx*1]
|   || ||   0x14603e5b8      mov   r12, r14
|   || ||   0x14603e5bb      mov   qword [var_d8h], r14
|   || ||   0x14603e5c0      mov   r8, rbx
|   || ||   0x14603e5c3      mov   rdx, qword [var_108h]
|   || ||   0x14603e5c8      mov   rcx, r15
|   || ||   0x14603e5cb      call  0x1473116de
|   || ||   0x14603e5d0      mov   qword [var_e0h], r14
|   ||,===< 0x14603e5d5      jmp   0x14603e5dc
|   ||||`-> 0x14603e5d7      mov   rdi, qword [var_e8h]
|   ||||    ; CODE XREF from fcn.14603e4e0 @ 0x14603e5d5
|   ||`---> 0x14603e5dc      mov   qword [var_108h], rdi
|   || |    0x14603e5e1      sub   r12, r15
|   || |    0x14603e5e4      mov   qword [var_100h], r12
|   || |    0x14603e5e9      mov   rdx, r12
|   || |    0x14603e5ec      lea   rcx, qword [var_a8h]
|   || |    0x14603e5f0      call  0x1435d0a80
|   || |    0x14603e5f5      nop
|   || |    0x14603e5f6      movaps xmm0, xmmword [var_108h]
|   || |    0x14603e5fb      movdqa xmmword [var_108h], xmm0
|   || |    0x14603e601      lea   rdx, qword [var_108h]
|   || |    0x14603e606      lea   rcx, qword [var_a8h]
|   || |    0x14603e60a      call  0x147393a70
|   || |    0x14603e60f      mov   rax, qword [var_f8h]
|   || |    0x14603e614      mov   rcx, qword [rax+0x58]
|   || |    0x14603e618      test  rcx, rcx
|   || |,=< 0x14603e61b      jz    0x14603e6d8
|   || ||   0x14603e621      mov   rax, qword [rcx]
|   || ||   0x14603e624      lea   r9, qword [var_a8h]
|   || ||   0x14603e628      mov   r8d, 0x01
|   || ||   0x14603e62e      lea   rdx, qword [var_c8h]
|   || ||   0x14603e633      call  qword [rax+0x40]                    ; 64
|   || ||   0x14603e636      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|   || ||   0x14603e63d      cmp   al, 0x06                            ; 6
|   ||,===< 0x14603e63f      jb    0x14603e6d8
|   |||||   0x14603e645      shr   rax, 0x08
|   |||||   0x14603e649      cmp   al, 0x04                            ; 4
|  ,======< 0x14603e64b      jb    0x14603e6d8
|  ||||||   0x14603e651      lea   rax, qword [0x148f72b50]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient message sent: {}"
|  ||||||   0x14603e658      mov   qword [var_108h], rax
|  ||||||   0x14603e65d      mov   qword [var_100h], 0x42              ; 'B'
|  ||||||                                                              ; [0x42:8]=-1 ; 66
|  ||||||   0x14603e666      cmp   qword [rsi+0x18], 0x10
| ,=======< 0x14603e66b      jb    0x14603e670
| |||||||   0x14603e66d      mov   rsi, qword [rsi]
| `-------> 0x14603e670      mov   rax, qword [r13]
|  ||||||   0x14603e674      mov   qword [var_f8h], rsi
|  ||||||   0x14603e679      mov   qword [var_f0h], rax
|  ||||||   0x14603e67e      movaps xmm0, xmmword [var_f8h]
|  ||||||   0x14603e683      movdqa xmmword [var_48h], xmm0
|  ||||||   0x14603e688      mov   qword [var_f8h], 0x0d               ; [0xd:8]=-1 ; 13
|  ||||||   0x14603e691      lea   rax, qword [var_48h]
|  ||||||   0x14603e695      mov   qword [var_f0h], rax
|  ||||||   0x14603e69a      movaps xmm0, xmmword [var_f8h]
|  ||||||   0x14603e69f      movdqa xmmword [var_f8h], xmm0
|  ||||||   0x14603e6a5      movaps xmm1, xmmword [var_108h]
|  ||||||   0x14603e6aa      movdqa xmmword [var_108h], xmm1
|  ||||||   0x14603e6b0      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|  ||||||   0x14603e6b7      movaps xmmword [var_c8h], xmm0
|  ||||||   0x14603e6bc      mov   byte [var_118h], 0x01
|  ||||||   0x14603e6c1      lea   r9, qword [var_f8h]
|  ||||||   0x14603e6c6      lea   r8, qword [var_108h]
|  ||||||   0x14603e6cb      mov   dl, 0x04
|  ||||||   0x14603e6cd      lea   rcx, qword [var_c8h]
|  ||||||   0x14603e6d2      call  0x1438602b0
|  ||||||   0x14603e6d7      nop
|  `--`-`-> 0x14603e6d8      movzx edx, byte [var_78h]
|   || |    0x14603e6dc      mov   rcx, qword [var_90h]
|   || |    0x14603e6e0      call  0x147393230
|   || |    0x14603e6e5      nop
|   || |    0x14603e6e6      mov   rcx, qword [var_a8h]
|   || |    0x14603e6ea      test  rcx, rcx
|   || |,=< 0x14603e6ed      jz    0x14603e731
|   || ||   0x14603e6ef      mov   rdx, qword [var_98h]
|   || ||   0x14603e6f3      sub   rdx, rcx
|   || ||   0x14603e6f6      mov   rax, rcx
|   || ||   0x14603e6f9      cmp   rdx, 0x1000
|   ||,===< 0x14603e700      jb    0x14603e71e
|   |||||   0x14603e702      add   rdx, 0x27                           ; 39
|   |||||   0x14603e706      mov   rcx, qword [rcx-0x08]
|   |||||   0x14603e70a      sub   rax, rcx
|   |||||   0x14603e70d      add   rax, 0xfffffffffffffff8
|   |||||   0x14603e711      cmp   rax, 0x1f                           ; 31
|  ,======< 0x14603e715      jbe   0x14603e71e
|  ||||||   0x14603e717      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|  ||||||   0x14603e71d      int3
|  `--`---> 0x14603e71e      call  0x14385bd60
|   || ||   0x14603e723      xor   eax, eax
|   || ||   0x14603e725      mov   qword [var_a8h], rax
|   || ||   0x14603e729      mov   qword [var_a0h], rax
|   || ||   0x14603e72d      mov   qword [var_98h], rax
|   || |`-> 0x14603e731      test  rdi, rdi
|   || |,=< 0x14603e734      jz    0x14603e76c
|   || ||   0x14603e736      sub   r14, rdi
|   || ||   0x14603e739      mov   rax, rdi
|   || ||   0x14603e73c      cmp   r14, 0x1000
|   ||,===< 0x14603e743      jb    0x14603e761
|   |||||   0x14603e745      add   r14, 0x27                           ; 39
|   |||||   0x14603e749      mov   rdi, qword [rdi-0x08]
|   |||||   0x14603e74d      sub   rax, rdi
|   |||||   0x14603e750      add   rax, 0xfffffffffffffff8
|   |||||   0x14603e754      cmp   rax, 0x1f                           ; 31
|  ,======< 0x14603e758      jbe   0x14603e761
|  |`-----> 0x14603e75a      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|  | ||||   0x14603e760      int3
|  `--`---> 0x14603e761      mov   rdx, r14
|    | ||   0x14603e764      mov   rcx, rdi
|    | ||   0x14603e767      call  0x14385bd60
|    | |`-> 0x14603e76c      mov   rcx, qword [rbp]
|    | |    0x14603e770      xor   rcx, rsp
|    | |    0x14603e773      call  0x14730fca0
|    | |    0x14603e778      lea   r11, qword [var_28h]
|    | |    0x14603e780      mov   rbx, qword [r11+0x38]
|    | |    0x14603e784      mov   rsi, qword [r11+0x40]
|    | |    0x14603e788      mov   rdi, qword [r11+0x48]
|    | |    0x14603e78c      mov   rsp, r11
|    | |    0x14603e78f      pop   r15
|    | |    0x14603e791      pop   r14
|    | |    0x14603e793      pop   r13
|    | |    0x14603e795      pop   r12
|    | |    0x14603e797      pop   rbp
|    | |    0x14603e798      ret
|    `----> 0x14603e799      call  0x1407417f0
|      |    0x14603e79e      int3
|      `--> 0x14603e79f      call  0x140741e60
\           0x14603e7a4      int3
