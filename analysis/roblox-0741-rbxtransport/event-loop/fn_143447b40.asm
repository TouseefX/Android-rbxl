/ fcn.143447b40(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_70h @ stack - 0x70
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_60h @ stack - 0x60
|           ; var int64_t var_58h @ stack - 0x58
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_18h @ stack + 0x18
|           0x143447b40      mov   qword [var_18h], rbx
|           0x143447b45      push  rbp
|           0x143447b46      push  rsi
|           0x143447b47      push  rdi
|           0x143447b48      push  r14
|           0x143447b4a      push  r15
|           0x143447b4c      sub   rsp, 0x70
|           0x143447b50      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x143447b57      xor   rax, rsp
|           0x143447b5a      mov   qword [var_38h], rax
|           0x143447b5f      mov   rsi, rdx                            ; arg2
|           0x143447b62      mov   rbx, rcx                            ; arg1
|           0x143447b65      mov   qword [var_70h], rdx                ; arg2
|           0x143447b6a      mov   r14d, dword [rcx]                   ; arg1
|           0x143447b6d      test  r14d, r14d
|       ,=< 0x143447b70      jnz   0x143447b7a
|       |   0x143447b72      call  0x147310e21
|       |   0x143447b77      mov   r14d, eax
|       `-> 0x143447b7a      mov   rbp, qword [rbx+0x40]
|           0x143447b7e      sub   rbp, qword [rbx+0x38]
|           0x143447b82      sar   rbp, 0x03
|           0x143447b86      xor   r15d, r15d
|           0x143447b89      mov   edi, r15d
|           0x143447b8c      test  r14d, r14d
|       ,=< 0x143447b8f      jle   0x143447bbe
|      .--> 0x143447b91      lea   rdx, qword [var_68h]
|      :|   0x143447b96      mov   rcx, rbx
|      :|   0x143447b99      call  0x143447830
|      :|   0x143447b9e      nop
|      :|   0x143447b9f      lea   rcx, qword [var_60h]
|      :|   0x143447ba4      call  0x14277c520
|      :|   0x143447ba9      test  al, al
|     ,===< 0x143447bab      jz    0x143447c25
|     |:|   0x143447bad      lea   rcx, qword [var_60h]
|     |:|   0x143447bb2      call  0x14073a8e0
|     |:|   0x143447bb7      inc   edi
|     |:|   0x143447bb9      cmp   edi, r14d
|     |`==< 0x143447bbc      jl    0x143447b91
|     | `-> 0x143447bbe      movsxd r14, r14d
|     |     0x143447bc1      test  r14, r14
|     | ,=< 0x143447bc4      jle   0x143447bea
|     | |   0x143447bc6      lea   rdi, qword [rbp*8]
|     | |   0x143447bce      nop
|     |.--> 0x143447bd0      mov   rdx, qword [rbx+0x38]
|     |:|   0x143447bd4      mov   rdx, qword [rdx+rdi*1]
|     |:|   0x143447bd8      mov   rcx, rbx
|     |:|   0x143447bdb      call  0x143447a70
|     |:|   0x143447be0      lea   rdi, qword [rdi+0x08]
|     |:|   0x143447be4      sub   r14, 0x01
|     |`==< 0x143447be8      jnz   0x143447bd0
|     | `-> 0x143447bea      mov   dword [rsi], r15d
|     |     0x143447bed      mov   qword [rsi+0x08], r15
|     |     0x143447bf1      mov   qword [rsi+0x18], r15
|     |     0x143447bf5      mov   qword [rsi+0x20], 0x0f              ; [0xf:8]=-1 ; 15
|     |     0x143447bfd      mov   byte [rsi+0x08], 0x00
|     |     ; CODE XREF from fcn.143447b40 @ 0x143447cd8
|     | .-> 0x143447c01      mov   rax, rsi
|     | :   0x143447c04      mov   rcx, qword [var_38h]
|     | :   0x143447c09      xor   rcx, rsp
|     | :   0x143447c0c      call  0x14730fca0
|     | :   0x143447c11      mov   rbx, qword [var_18h]
|     | :   0x143447c19      add   rsp, 0x70
|     | :   0x143447c1d      pop   r15
|     | :   0x143447c1f      pop   r14
|     | :   0x143447c21      pop   rdi
|     | :   0x143447c22      pop   rsi
|     | :   0x143447c23      pop   rbp
|     | :   0x143447c24      ret
|     `---> 0x143447c25      lea   r15, qword [rbx+0x30]
|       :   0x143447c29      mov   r14, qword [r15+0x10]
|       :   0x143447c2d      mov   rdx, qword [r15+0x08]
|       :   0x143447c31      mov   rcx, r14
|       :   0x143447c34      sub   rcx, rdx
|       :   0x143447c37      sar   rcx, 0x03
|       :   0x143447c3b      cmp   rbp, rcx
|      ,==< 0x143447c3e      jnb   0x143447c77
|      |:   0x143447c40      lea   rbp, qword [rdx+rbp*8]
|      |:   0x143447c44      mov   rbx, rbp
|      |:   0x143447c47      cmp   rbp, r14
|     ,===< 0x143447c4a      jz    0x143447c71
|     ||:   0x143447c4c      nop   dword [rax], eax
|    .----> 0x143447c50      mov   rdi, qword [rbx]
|    :||:   0x143447c53      test  rdi, rdi
|   ,=====< 0x143447c56      jz    0x143447c68
|   |:||:   0x143447c58      mov   rcx, rdi
|   |:||:   0x143447c5b      call  0x143445bd0
|   |:||:   0x143447c60      mov   rcx, rdi
|   |:||:   0x143447c63      call  0x14385bd60
|   `-----> 0x143447c68      add   rbx, 0x08
|    :||:   0x143447c6c      cmp   rbx, r14
|    `====< 0x143447c6f      jnz   0x143447c50
|     `---> 0x143447c71      mov   qword [r15+0x10], rbp
|     ,===< 0x143447c75      jmp   0x143447cb9
|    ,=`--> 0x143447c77      jbe   0x143447cb9
|    || :   0x143447c79      mov   rax, qword [r15+0x18]
|    || :   0x143447c7d      sub   rax, rdx
|    || :   0x143447c80      sar   rax, 0x03
|    || :   0x143447c84      cmp   rbp, rax
|    ||,==< 0x143447c87      jbe   0x143447c9b
|    |||:   0x143447c89      lea   r8, qword [var_78h]
|    |||:   0x143447c8e      mov   rdx, rbp
|    |||:   0x143447c91      mov   rcx, r15
|    |||:   0x143447c94      call  0x143446ea0
|   ,=====< 0x143447c99      jmp   0x143447cb9
|   |||`--> 0x143447c9b      sub   rbp, rcx
|   |||,==< 0x143447c9e      jz    0x143447cb5
|   ||||:   0x143447ca0      mov   r8, rbp
|   ||||:   0x143447ca3      shl   r8, 0x03
|   ||||:   0x143447ca7      xor   edx, edx
|   ||||:   0x143447ca9      mov   rcx, r14
|   ||||:   0x143447cac      call  0x1473116e4
|   ||||:   0x143447cb1      lea   r14, qword [r14+rbp*8]
|   |||`--> 0x143447cb5      mov   qword [r15+0x10], r14
|   ||| :   ; CODE XREFS from fcn.143447b40 @ 0x143447c75, 0x143447c99
|   ```---> 0x143447cb9      mov   ecx, dword [var_60h]
|       :   0x143447cbd      mov   dword [rsi], ecx
|       :   0x143447cbf      lea   rcx, qword [rsi+0x08]
|       :   0x143447cc3      lea   rdx, qword [var_58h]
|       :   0x143447cc8      call  0x140739200
|       :   0x143447ccd      nop
|       :   0x143447cce      lea   rcx, qword [var_60h]
|       :   0x143447cd3      call  0x14073a8e0
\       `=< 0x143447cd8      jmp   0x143447c01
