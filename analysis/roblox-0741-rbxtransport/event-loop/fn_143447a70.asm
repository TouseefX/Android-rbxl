/ fcn.143447a70(int64_t arg1, int64_t arg2, int64_t arg_38h);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_20h @ stack - 0x20
|           ; var int64_t var_8h @ stack + 0x8
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           ; arg int64_t arg_38h @ stack + 0x38
|           0x143447a70      mov   qword [var_10h], rbx
|           0x143447a75      mov   qword [var_18h], rsi
|           0x143447a7a      push  rdi
|           0x143447a7b      sub   rsp, 0x40
|           0x143447a7f      mov   rbx, rcx                            ; arg1
|           0x143447a82      mov   rax, qword [rcx+0x68]               ; arg1
|           0x143447a86      sub   rax, qword [rcx+0x60]               ; arg1
|           0x143447a8a      sar   rax, 0x04
|           0x143447a8e      add   rcx, 0x58                           ; 88 ; arg1
|           0x143447a92      mov   qword [var_20h + 0x8], rdx          ; arg2
|           0x143447a97      mov   qword [var_20h + 0x10], rax
|           0x143447a9c      mov   rdi, qword [rbx+0x68]
|           0x143447aa0      cmp   rdi, qword [rbx+0x70]
|       ,=< 0x143447aa4      jz    0x143447b04
|       |   0x143447aa6      mov   ecx, 0x10                           ; 16
|       |   0x143447aab      call  0x14385bce0
|       |   0x143447ab0      test  rax, rax
|      ,==< 0x143447ab3      jz    0x143447abd
|      ||   0x143447ab5      movups xmm0, xmmword [arg_38h]
|      ||   0x143447aba      movups xmmword [rax], xmm0
|      `--> 0x143447abd      mov   qword [var_8h], rax
|       |   0x143447ac2      lea   rsi, qword [rdi+0x08]
|       |   0x143447ac6      mov   qword [var_20h], rsi
|       |   0x143447acb      mov   dword [var_28h], 0x00
|       |   0x143447ad3      mov   r9, rax
|       |   0x143447ad6      lea   r8, qword [0x143446d90]
|       |   0x143447add      xor   edx, edx
|       |   0x143447adf      xor   ecx, ecx
|       |   0x143447ae1      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__beginthreadex] ; [0x148442558:8]=0xc35a274 ; "t\xa25\f"
|       |   0x143447ae7      mov   qword [rdi], rax
|       |   0x143447aea      test  rax, rax
|      ,==< 0x143447aed      jz    0x143447b21
|      ||   0x143447aef      add   qword [rbx+0x68], 0x10              ; [0x10:8]=-1 ; 16
|      ||   0x143447af4      mov   rbx, qword [var_10h]
|      ||   0x143447af9      mov   rsi, qword [var_18h]
|      ||   0x143447afe      add   rsp, 0x40
|      ||   0x143447b02      pop   rdi
|      ||   0x143447b03      ret
|      |`-> 0x143447b04      lea   r8, qword [var_20h + 0x8]
|      |    0x143447b09      mov   rdx, rdi
|      |    0x143447b0c      call  0x143446a10
|      |    0x143447b11      mov   rbx, qword [var_10h]
|      |    0x143447b16      mov   rsi, qword [var_18h]
|      |    0x143447b1b      add   rsp, 0x40
|      |    0x143447b1f      pop   rdi
|      |    0x143447b20      ret
|      `--> 0x143447b21      mov   dword [rsi], 0x00
|           0x143447b27      mov   ecx, 0x06
|           0x143447b2c      call  0x1473107c4
\           0x143447b31      int3
