/ fcn.14344ba20(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_40h @ stack - 0x40
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_8h @ stack - 0x8
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           ; var int64_t var_20h @ stack + 0x20
|           0x14344ba20      mov   qword [var_10h], rbx
|           0x14344ba25      mov   qword [var_18h], rbp
|           0x14344ba2a      mov   qword [var_20h], rsi
|           0x14344ba2f      push  rdi
|           0x14344ba30      sub   rsp, 0x60
|           0x14344ba34      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14344ba3b      xor   rax, rsp
|           0x14344ba3e      mov   qword [var_38h + 0x20], rax
|           0x14344ba43      mov   rsi, rcx                            ; arg1
|           0x14344ba46      mov   qword [var_48h], rcx                ; arg1
|           0x14344ba4b      lea   rax, qword [0x1489f9ff8]
|           0x14344ba52      mov   qword [rcx], rax                    ; arg1
|           0x14344ba55      add   rcx, 0x08                           ; arg1
|           0x14344ba59      mov   byte [rcx], 0x00                    ; arg1
|           0x14344ba5c      xor   ebp, ebp
|           0x14344ba5e      mov   dword [rsi+0x10], ebp
|           0x14344ba61      mov   qword [rsi+0x18], rbp
|           0x14344ba65      mov   qword [rsi+0x28], rbp
|           0x14344ba69      mov   qword [rsi+0x30], 0x0f              ; [0xf:8]=-1 ; 15
|           0x14344ba71      lea   rdx, qword [var_40h]
|           0x14344ba76      call  0x143453960
|           0x14344ba7b      lea   rdi, qword [rax+0x08]
|           0x14344ba7f      mov   eax, dword [rax]
|           0x14344ba81      mov   dword [rsi+0x10], eax
|           0x14344ba84      lea   rbx, qword [rsi+0x18]
|           0x14344ba88      cmp   rbx, rdi
|       ,=< 0x14344ba8b      jz    0x14344baef
|       |   0x14344ba8d      mov   rdx, qword [rbx+0x18]
|       |   0x14344ba91      cmp   rdx, 0x10                           ; 16
|      ,==< 0x14344ba95      jb    0x14344bac3
|      ||   0x14344ba97      mov   rcx, qword [rbx]
|      ||   0x14344ba9a      inc   rdx
|      ||   0x14344ba9d      cmp   rdx, 0x1000
|     ,===< 0x14344baa4      jb    0x14344babe
|     |||   0x14344baa6      add   rdx, 0x27                           ; 39
|     |||   0x14344baaa      mov   r8, qword [rcx-0x08]
|     |||   0x14344baae      sub   rcx, r8
|     |||   0x14344bab1      lea   rax, qword [rcx-0x08]
|     |||   0x14344bab5      cmp   rax, 0x1f                           ; 31
|    ,====< 0x14344bab9      jnbe  0x14344bb23
|    ||||   0x14344babb      mov   rcx, r8
|    |`---> 0x14344babe      call  0x14385bd60
|    | `--> 0x14344bac3      mov   qword [rbx+0x10], rbp
|    |  |   0x14344bac7      mov   qword [rbx+0x18], 0x0f              ; [0xf:8]=-1 ; 15
|    |  |   0x14344bacf      mov   byte [rbx], 0x00
|    |  |   0x14344bad2      movups xmm0, xmmword [rdi]
|    |  |   0x14344bad5      movups xmmword [rbx], xmm0
|    |  |   0x14344bad8      movups xmm1, xmmword [rdi+0x10]
|    |  |   0x14344badc      movups xmmword [rbx+0x10], xmm1
|    |  |   0x14344bae0      mov   qword [rdi+0x10], rbp
|    |  |   0x14344bae4      mov   qword [rdi+0x18], 0x0f              ; [0xf:8]=-1 ; 15
|    |  |   0x14344baec      mov   byte [rdi], 0x00
|    |  `-> 0x14344baef      mov   rdx, qword [var_38h + 0x18]
|    |      0x14344baf4      cmp   rdx, 0x10                           ; 16
|    |  ,=< 0x14344baf8      jb    0x14344bb30
|    |  |   0x14344bafa      inc   rdx
|    |  |   0x14344bafd      mov   rcx, qword [var_38h]
|    |  |   0x14344bb02      mov   rax, rcx
|    |  |   0x14344bb05      cmp   rdx, 0x1000
|    | ,==< 0x14344bb0c      jb    0x14344bb2a
|    | ||   0x14344bb0e      add   rdx, 0x27                           ; 39
|    | ||   0x14344bb12      mov   rcx, qword [rcx-0x08]
|    | ||   0x14344bb16      sub   rax, rcx
|    | ||   0x14344bb19      add   rax, 0xfffffffffffffff8
|    | ||   0x14344bb1d      cmp   rax, 0x1f                           ; 31
|    |,===< 0x14344bb21      jbe   0x14344bb2a
|    `----> 0x14344bb23      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|     |||   0x14344bb29      int3
|     ``--> 0x14344bb2a      call  0x14385bd60
|       |   0x14344bb2f      nop
|       `-> 0x14344bb30      mov   rax, rsi
|           0x14344bb33      mov   rcx, qword [var_38h + 0x20]
|           0x14344bb38      xor   rcx, rsp
|           0x14344bb3b      call  0x14730fca0
|           0x14344bb40      lea   r11, qword [var_8h]
|           0x14344bb45      mov   rbx, qword [r11+0x18]
|           0x14344bb49      mov   rbp, qword [r11+0x20]
|           0x14344bb4d      mov   rsi, qword [r11+0x28]
|           0x14344bb51      mov   rsp, r11
|           0x14344bb54      pop   rdi
\           0x14344bb55      ret
