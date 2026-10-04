/ fcn.143446d90(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_60h @ stack - 0x60
|           ; var int64_t var_58h @ stack - 0x58
|           ; var int64_t var_50h @ stack - 0x50
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_30h @ stack - 0x30
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_18h @ stack - 0x18
|           0x143446d90      push  rbx
|           0x143446d92      sub   rsp, 0x80
|           0x143446d99      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x143446da0      xor   rax, rsp
|           0x143446da3      mov   qword [var_18h], rax
|           0x143446da8      mov   rbx, rcx                            ; arg1
|           0x143446dab      lea   rax, qword [0x1489f9ba8]            ; "RBX IoEvLoop {}"
|           0x143446db2      mov   qword [var_58h], rax
|           0x143446db7      mov   qword [var_50h], 0x0f               ; [0xf:8]=-1 ; 15
|           0x143446dc0      mov   rax, qword [rcx+0x08]               ; arg1
|           0x143446dc4      mov   qword [var_68h], rax
|           0x143446dc9      movaps xmm0, xmmword [var_68h]
|           0x143446dce      movdqa xmmword [var_28h], xmm0
|           0x143446dd4      mov   qword [var_68h], 0x04
|           0x143446ddd      lea   rax, qword [var_28h]
|           0x143446de2      mov   qword [var_60h], rax
|           0x143446de7      movaps xmm0, xmmword [var_68h]
|           0x143446dec      movdqa xmmword [var_68h], xmm0
|           0x143446df2      movaps xmm1, xmmword [var_58h]
|           0x143446df7      movdqa xmmword [var_58h], xmm1
|           0x143446dfd      lea   r8, qword [var_68h]
|           0x143446e02      lea   rdx, qword [var_58h]
|           0x143446e07      lea   rcx, qword [var_48h]
|           0x143446e0c      call  fcn.142a338a0
|           0x143446e11      lea   rcx, qword [var_48h]
|           0x143446e16      cmp   qword [var_30h], 0x10
|           0x143446e1c      cmovnb rcx, qword [var_48h]
|           0x143446e22      call  fcn.1438688a0
|           0x143446e27      mov   rdx, qword [var_30h]
|           0x143446e2c      cmp   rdx, 0x10                           ; 16
|       ,=< 0x143446e30      jb    0x143446e67
|       |   0x143446e32      inc   rdx
|       |   0x143446e35      mov   rcx, qword [var_48h]
|       |   0x143446e3a      mov   rax, rcx
|       |   0x143446e3d      cmp   rdx, 0x1000
|      ,==< 0x143446e44      jb    0x143446e62
|      ||   0x143446e46      add   rdx, 0x27                           ; 39
|      ||   0x143446e4a      mov   rcx, qword [rcx-0x08]
|      ||   0x143446e4e      sub   rax, rcx
|      ||   0x143446e51      add   rax, 0xfffffffffffffff8
|      ||   0x143446e55      cmp   rax, 0x1f                           ; 31
|     ,===< 0x143446e59      jbe   0x143446e62
|     |||   0x143446e5b      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|     |||   0x143446e61      int3
|     ``--> 0x143446e62      call  fcn.14385bd60
|       `-> 0x143446e67      mov   rcx, qword [rbx]
|           0x143446e6a      call  fcn.1434464d0
|           0x143446e6f      call  sub.MSVCP140.dll__Cnd_do_broadcast_at_thread_exit
|           0x143446e74      mov   rcx, rbx
|           0x143446e77      call  fcn.14385bd60
|           0x143446e7c      xor   eax, eax
|           0x143446e7e      mov   rcx, qword [var_18h]
|           0x143446e83      xor   rcx, rsp
|           0x143446e86      call  fcn.14730fca0
|           0x143446e8b      add   rsp, 0x80
|           0x143446e92      pop   rbx
\           0x143446e93      ret
