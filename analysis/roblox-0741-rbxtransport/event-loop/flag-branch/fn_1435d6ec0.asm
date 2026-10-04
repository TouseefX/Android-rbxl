/ fcn.1435d6ec0(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_a0h @ stack - 0xa0
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_18h @ stack - 0x18
|           ; var int64_t var_10h @ stack - 0x10
|           ; var int64_t var_8h @ stack - 0x8
|           0x1435d6ec0      mov   r11, rsp
|           0x1435d6ec3      mov   qword [r11+0x10], rbx
|           0x1435d6ec7      mov   qword [r11+0x18], rbp
|           0x1435d6ecb      mov   qword [r11+0x20], rsi
|           0x1435d6ecf      push  rdi
|           0x1435d6ed0      sub   rsp, 0xc0
|           0x1435d6ed7      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x1435d6ede      xor   rax, rsp
|           0x1435d6ee1      mov   qword [var_10h], rax
|           0x1435d6ee9      mov   ebp, edx                            ; arg2
|           0x1435d6eeb      mov   rbx, rcx                            ; arg1
|           0x1435d6eee      mov   r8d, 0x01
|           0x1435d6ef4      lea   rdx, qword [0x148aa7610]            ; "~SharedConnectionRegistry"
|           0x1435d6efb      lea   rcx, qword [r11-0x18]
|           0x1435d6eff      call  0x1427c8d90
|           0x1435d6f04      mov   qword [var_a8h], rbx
|           0x1435d6f09      lea   rax, qword [var_18h]
|           0x1435d6f11      mov   qword [var_a0h], rax
|           0x1435d6f16      movups xmm0, xmmword [var_a8h]
|           0x1435d6f1b      movaps xmmword [var_98h], xmm0
|           0x1435d6f20      lea   rax, qword [0x148aa7670]
|           0x1435d6f27      and   rax, 0xfffffffffffffffb
|           0x1435d6f2b      or    rax, 0x02
|           0x1435d6f2f      mov   qword [var_28h], rax
|           0x1435d6f37      lea   rdx, qword [var_98h]
|           0x1435d6f3c      mov   rcx, qword [rbx]
|           0x1435d6f3f      call  0x143445f60
|           0x1435d6f44      lea   rcx, qword [var_18h]
|           0x1435d6f4c      call  0x1427c8dd0
|           0x1435d6f51      lea   rcx, qword [var_18h]
|           0x1435d6f59      call  0x1427c8200
|           0x1435d6f5e      nop
|           0x1435d6f5f      mov   rdi, qword [rbx+0x78]
|           0x1435d6f63      mov   rax, rdi
|           0x1435d6f66      and   rax, 0xfffffffffffffffc
|           0x1435d6f6a      mov   rcx, rax
|           0x1435d6f6d      and   rcx, 0xfffffffffffffff8
|       ,=< 0x1435d6f71      jz    0x1435d6fa5
|       |   0x1435d6f73      sar   dil, 0x01
|       |   0x1435d6f76      and   dil, 0x01
|       |   0x1435d6f7a      sar   al, 0x02
|       |   0x1435d6f7d      not   al
|       |   0x1435d6f7f      test  al, 0x01                            ; 1
|      ,==< 0x1435d6f81      jnz   0x1435d6f96
|      ||   0x1435d6f83      mov   rax, qword [rcx+0x10]
|      ||   0x1435d6f87      test  dil, dil
|      ||   0x1435d6f8a      lea   rcx, qword [rbx+0x60]
|     ,===< 0x1435d6f8e      jnz   0x1435d6f94
|     |||   0x1435d6f90      mov   rcx, qword [rbx+0x60]
|     `---> 0x1435d6f94      call  rax
|      `--> 0x1435d6f96      test  dil, dil
|      ,==< 0x1435d6f99      jnz   0x1435d6fa5
|      ||   0x1435d6f9b      mov   rcx, qword [rbx+0x60]
|      ||   0x1435d6f9f      call  0x14385ca60
|      ||   0x1435d6fa4      nop
|      ``-> 0x1435d6fa5      mov   rdi, qword [rbx+0x58]
|           0x1435d6fa9      mov   rax, rdi
|           0x1435d6fac      and   rax, 0xfffffffffffffffc
|           0x1435d6fb0      mov   rcx, rax
|           0x1435d6fb3      and   rcx, 0xfffffffffffffff8
|       ,=< 0x1435d6fb7      jz    0x1435d6feb
|       |   0x1435d6fb9      sar   dil, 0x01
|       |   0x1435d6fbc      and   dil, 0x01
|       |   0x1435d6fc0      sar   al, 0x02
|       |   0x1435d6fc3      not   al
|       |   0x1435d6fc5      test  al, 0x01                            ; 1
|      ,==< 0x1435d6fc7      jnz   0x1435d6fdc
|      ||   0x1435d6fc9      mov   rax, qword [rcx+0x10]
|      ||   0x1435d6fcd      test  dil, dil
|      ||   0x1435d6fd0      lea   rcx, qword [rbx+0x40]
|     ,===< 0x1435d6fd4      jnz   0x1435d6fda
|     |||   0x1435d6fd6      mov   rcx, qword [rbx+0x40]
|     `---> 0x1435d6fda      call  rax
|      `--> 0x1435d6fdc      test  dil, dil
|      ,==< 0x1435d6fdf      jnz   0x1435d6feb
|      ||   0x1435d6fe1      mov   rcx, qword [rbx+0x40]
|      ||   0x1435d6fe5      call  0x14385ca60
|      ||   0x1435d6fea      nop
|      ``-> 0x1435d6feb      lea   rcx, qword [rbx+0x18]
|           0x1435d6fef      call  0x1435d4e60
|           0x1435d6ff4      lea   rcx, qword [rbx+0x08]
|           0x1435d6ff8      call  0x1427c8200
|           0x1435d6ffd      nop
|           0x1435d6ffe      test  bpl, 0x01                           ; 1
|       ,=< 0x1435d7002      jz    0x1435d700c
|       |   0x1435d7004      mov   rcx, rbx
|       |   0x1435d7007      call  0x14385bd60
|       `-> 0x1435d700c      mov   rax, rbx
|           0x1435d700f      mov   rcx, qword [var_10h]
|           0x1435d7017      xor   rcx, rsp
|           0x1435d701a      call  0x14730fca0
|           0x1435d701f      lea   r11, qword [var_8h]
|           0x1435d7027      mov   rbx, qword [r11+0x18]
|           0x1435d702b      mov   rbp, qword [r11+0x20]
|           0x1435d702f      mov   rsi, qword [r11+0x28]
|           0x1435d7033      mov   rsp, r11
|           0x1435d7036      pop   rdi
\           0x1435d7037      ret
