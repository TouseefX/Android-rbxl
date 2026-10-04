/ fcn.1435d6d40(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_a0h @ stack - 0xa0
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_18h @ stack - 0x18
|           ; var int64_t var_10h @ stack - 0x10
|           ; var int64_t var_8h @ stack - 0x8
|           0x1435d6d40      mov   r11, rsp
|           0x1435d6d43      mov   qword [r11+0x10], rbx
|           0x1435d6d47      mov   qword [r11+0x18], rbp
|           0x1435d6d4b      mov   qword [r11+0x20], rsi
|           0x1435d6d4f      push  rdi
|           0x1435d6d50      sub   rsp, 0xc0
|           0x1435d6d57      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x1435d6d5e      xor   rax, rsp
|           0x1435d6d61      mov   qword [var_10h], rax
|           0x1435d6d69      mov   ebp, edx                            ; arg2
|           0x1435d6d6b      mov   rbx, rcx                            ; arg1
|           0x1435d6d6e      mov   r8d, 0x01
|           0x1435d6d74      lea   rdx, qword [0x148aa7610]            ; "~SharedConnectionRegistry"
|           0x1435d6d7b      lea   rcx, qword [r11-0x18]
|           0x1435d6d7f      call  0x1427c8d90
|           0x1435d6d84      mov   qword [var_a8h], rbx
|           0x1435d6d89      lea   rax, qword [var_18h]
|           0x1435d6d91      mov   qword [var_a0h], rax
|           0x1435d6d96      movups xmm0, xmmword [var_a8h]
|           0x1435d6d9b      movaps xmmword [var_98h], xmm0
|           0x1435d6da0      lea   rax, qword [0x148aa7660]
|           0x1435d6da7      and   rax, 0xfffffffffffffffb
|           0x1435d6dab      or    rax, 0x02
|           0x1435d6daf      mov   qword [var_28h], rax
|           0x1435d6db7      lea   rdx, qword [var_98h]
|           0x1435d6dbc      mov   rcx, qword [rbx]
|           0x1435d6dbf      call  0x143445f60
|           0x1435d6dc4      lea   rcx, qword [var_18h]
|           0x1435d6dcc      call  0x1427c8dd0
|           0x1435d6dd1      lea   rcx, qword [var_18h]
|           0x1435d6dd9      call  0x1427c8200
|           0x1435d6dde      nop
|           0x1435d6ddf      mov   rdi, qword [rbx+0x78]
|           0x1435d6de3      mov   rax, rdi
|           0x1435d6de6      and   rax, 0xfffffffffffffffc
|           0x1435d6dea      mov   rcx, rax
|           0x1435d6ded      and   rcx, 0xfffffffffffffff8
|       ,=< 0x1435d6df1      jz    0x1435d6e25
|       |   0x1435d6df3      sar   dil, 0x01
|       |   0x1435d6df6      and   dil, 0x01
|       |   0x1435d6dfa      sar   al, 0x02
|       |   0x1435d6dfd      not   al
|       |   0x1435d6dff      test  al, 0x01                            ; 1
|      ,==< 0x1435d6e01      jnz   0x1435d6e16
|      ||   0x1435d6e03      mov   rax, qword [rcx+0x10]
|      ||   0x1435d6e07      test  dil, dil
|      ||   0x1435d6e0a      lea   rcx, qword [rbx+0x60]
|     ,===< 0x1435d6e0e      jnz   0x1435d6e14
|     |||   0x1435d6e10      mov   rcx, qword [rbx+0x60]
|     `---> 0x1435d6e14      call  rax
|      `--> 0x1435d6e16      test  dil, dil
|      ,==< 0x1435d6e19      jnz   0x1435d6e25
|      ||   0x1435d6e1b      mov   rcx, qword [rbx+0x60]
|      ||   0x1435d6e1f      call  0x14385ca60
|      ||   0x1435d6e24      nop
|      ``-> 0x1435d6e25      mov   rdi, qword [rbx+0x58]
|           0x1435d6e29      mov   rax, rdi
|           0x1435d6e2c      and   rax, 0xfffffffffffffffc
|           0x1435d6e30      mov   rcx, rax
|           0x1435d6e33      and   rcx, 0xfffffffffffffff8
|       ,=< 0x1435d6e37      jz    0x1435d6e6b
|       |   0x1435d6e39      sar   dil, 0x01
|       |   0x1435d6e3c      and   dil, 0x01
|       |   0x1435d6e40      sar   al, 0x02
|       |   0x1435d6e43      not   al
|       |   0x1435d6e45      test  al, 0x01                            ; 1
|      ,==< 0x1435d6e47      jnz   0x1435d6e5c
|      ||   0x1435d6e49      mov   rax, qword [rcx+0x10]
|      ||   0x1435d6e4d      test  dil, dil
|      ||   0x1435d6e50      lea   rcx, qword [rbx+0x40]
|     ,===< 0x1435d6e54      jnz   0x1435d6e5a
|     |||   0x1435d6e56      mov   rcx, qword [rbx+0x40]
|     `---> 0x1435d6e5a      call  rax
|      `--> 0x1435d6e5c      test  dil, dil
|      ,==< 0x1435d6e5f      jnz   0x1435d6e6b
|      ||   0x1435d6e61      mov   rcx, qword [rbx+0x40]
|      ||   0x1435d6e65      call  0x14385ca60
|      ||   0x1435d6e6a      nop
|      ``-> 0x1435d6e6b      lea   rcx, qword [rbx+0x18]
|           0x1435d6e6f      call  0x1435d4df0
|           0x1435d6e74      lea   rcx, qword [rbx+0x08]
|           0x1435d6e78      call  0x1427c8200
|           0x1435d6e7d      nop
|           0x1435d6e7e      test  bpl, 0x01                           ; 1
|       ,=< 0x1435d6e82      jz    0x1435d6e8c
|       |   0x1435d6e84      mov   rcx, rbx
|       |   0x1435d6e87      call  0x14385bd60
|       `-> 0x1435d6e8c      mov   rax, rbx
|           0x1435d6e8f      mov   rcx, qword [var_10h]
|           0x1435d6e97      xor   rcx, rsp
|           0x1435d6e9a      call  0x14730fca0
|           0x1435d6e9f      lea   r11, qword [var_8h]
|           0x1435d6ea7      mov   rbx, qword [r11+0x18]
|           0x1435d6eab      mov   rbp, qword [r11+0x20]
|           0x1435d6eaf      mov   rsi, qword [r11+0x28]
|           0x1435d6eb3      mov   rsp, r11
|           0x1435d6eb6      pop   rdi
\           0x1435d6eb7      ret
