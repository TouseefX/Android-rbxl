/ fcn.143465e90(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_40h @ stack - 0x40
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_8h @ stack - 0x8
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           ; var int64_t var_20h @ stack + 0x20
|           0x143465e90      mov   qword [var_10h], rbx
|           0x143465e95      mov   qword [var_18h], rbp
|           0x143465e9a      mov   qword [var_20h], rsi
|           0x143465e9f      push  rdi
|           0x143465ea0      sub   rsp, 0x60
|           0x143465ea4      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x143465eab      xor   rax, rsp
|           0x143465eae      mov   qword [var_38h + 0x20], rax
|           0x143465eb3      mov   rsi, rcx                            ; arg1
|           0x143465eb6      mov   qword [var_48h], rcx                ; arg1
|           0x143465ebb      lea   rax, qword [0x1489fb180]
|           0x143465ec2      mov   qword [rcx], rax                    ; arg1
|           0x143465ec5      add   rcx, 0x08                           ; arg1
|           0x143465ec9      mov   byte [rcx], 0x00                    ; arg1
|           0x143465ecc      xor   ebp, ebp
|           0x143465ece      mov   dword [rsi+0x10], ebp
|           0x143465ed1      mov   qword [rsi+0x18], rbp
|           0x143465ed5      mov   qword [rsi+0x28], rbp
|           0x143465ed9      mov   qword [rsi+0x30], 0x0f              ; [0xf:8]=-1 ; 15
|           0x143465ee1      lea   rdx, qword [var_40h]
|           0x143465ee6      call  0x143453960
|           0x143465eeb      lea   rdi, qword [rax+0x08]
|           0x143465eef      mov   eax, dword [rax]
|           0x143465ef1      mov   dword [rsi+0x10], eax
|           0x143465ef4      lea   rbx, qword [rsi+0x18]
|           0x143465ef8      cmp   rbx, rdi
|       ,=< 0x143465efb      jz    0x143465f5f
|       |   0x143465efd      mov   rdx, qword [rbx+0x18]
|       |   0x143465f01      cmp   rdx, 0x10                           ; 16
|      ,==< 0x143465f05      jb    0x143465f33
|      ||   0x143465f07      mov   rcx, qword [rbx]
|      ||   0x143465f0a      inc   rdx
|      ||   0x143465f0d      cmp   rdx, 0x1000
|     ,===< 0x143465f14      jb    0x143465f2e
|     |||   0x143465f16      add   rdx, 0x27                           ; 39
|     |||   0x143465f1a      mov   r8, qword [rcx-0x08]
|     |||   0x143465f1e      sub   rcx, r8
|     |||   0x143465f21      lea   rax, qword [rcx-0x08]
|     |||   0x143465f25      cmp   rax, 0x1f                           ; 31
|    ,====< 0x143465f29      jnbe  0x143465f93
|    ||||   0x143465f2b      mov   rcx, r8
|    |`---> 0x143465f2e      call  0x14385bd60
|    | `--> 0x143465f33      mov   qword [rbx+0x10], rbp
|    |  |   0x143465f37      mov   qword [rbx+0x18], 0x0f              ; [0xf:8]=-1 ; 15
|    |  |   0x143465f3f      mov   byte [rbx], 0x00
|    |  |   0x143465f42      movups xmm0, xmmword [rdi]
|    |  |   0x143465f45      movups xmmword [rbx], xmm0
|    |  |   0x143465f48      movups xmm1, xmmword [rdi+0x10]
|    |  |   0x143465f4c      movups xmmword [rbx+0x10], xmm1
|    |  |   0x143465f50      mov   qword [rdi+0x10], rbp
|    |  |   0x143465f54      mov   qword [rdi+0x18], 0x0f              ; [0xf:8]=-1 ; 15
|    |  |   0x143465f5c      mov   byte [rdi], 0x00
|    |  `-> 0x143465f5f      mov   rdx, qword [var_38h + 0x18]
|    |      0x143465f64      cmp   rdx, 0x10                           ; 16
|    |  ,=< 0x143465f68      jb    0x143465fa0
|    |  |   0x143465f6a      inc   rdx
|    |  |   0x143465f6d      mov   rcx, qword [var_38h]
|    |  |   0x143465f72      mov   rax, rcx
|    |  |   0x143465f75      cmp   rdx, 0x1000
|    | ,==< 0x143465f7c      jb    0x143465f9a
|    | ||   0x143465f7e      add   rdx, 0x27                           ; 39
|    | ||   0x143465f82      mov   rcx, qword [rcx-0x08]
|    | ||   0x143465f86      sub   rax, rcx
|    | ||   0x143465f89      add   rax, 0xfffffffffffffff8
|    | ||   0x143465f8d      cmp   rax, 0x1f                           ; 31
|    |,===< 0x143465f91      jbe   0x143465f9a
|    `----> 0x143465f93      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|     |||   0x143465f99      int3
|     ``--> 0x143465f9a      call  0x14385bd60
|       |   0x143465f9f      nop
|       `-> 0x143465fa0      mov   rax, rsi
|           0x143465fa3      mov   rcx, qword [var_38h + 0x20]
|           0x143465fa8      xor   rcx, rsp
|           0x143465fab      call  0x14730fca0
|           0x143465fb0      lea   r11, qword [var_8h]
|           0x143465fb5      mov   rbx, qword [r11+0x18]
|           0x143465fb9      mov   rbp, qword [r11+0x20]
|           0x143465fbd      mov   rsi, qword [r11+0x28]
|           0x143465fc1      mov   rsp, r11
|           0x143465fc4      pop   rdi
\           0x143465fc5      ret
