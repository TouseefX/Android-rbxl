/ fcn.14603e370(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_70h @ stack - 0x70
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_60h @ stack - 0x60
|           ; var int64_t var_58h @ stack - 0x58
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_18h @ stack + 0x18
|           0x14603e370      mov   qword [var_18h], rbx
|           0x14603e375      push  rbp
|           0x14603e376      push  rsi
|           0x14603e377      push  rdi
|           0x14603e378      sub   rsp, 0x90
|           0x14603e37f      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14603e386      xor   rax, rsp
|           0x14603e389      mov   qword [var_28h], rax
|           0x14603e391      mov   rbp, rdx                            ; arg2
|           0x14603e394      mov   rdi, rcx                            ; arg1
|           0x14603e397      lea   rbx, qword [rcx+0x50]               ; arg1
|           0x14603e39b      mov   qword [var_58h], rbx
|           0x14603e3a0      mov   rcx, rbx
|           0x14603e3a3      call  fcn.1427c8210
|           0x14603e3a8      nop
|           0x14603e3a9      cmp   byte [rdi+0x82], 0x00
|       ,=< 0x14603e3b0      jz    0x14603e3c2
|       |   0x14603e3b2      mov   rdx, rbp
|       |   0x14603e3b5      mov   rcx, rdi
|       |   0x14603e3b8      call  0x14603e4e0
|      ,==< 0x14603e3bd      jmp   0x14603e4a9
|      |`-> 0x14603e3c2      mov   rax, qword [rdi+0x90]
|      |    0x14603e3c9      cmp   rax, qword [rdi+0x98]
|      |,=< 0x14603e3d0      jz    0x14603e3e7
|      ||   0x14603e3d2      mov   rdx, rbp
|      ||   0x14603e3d5      mov   rcx, rax
|      ||   0x14603e3d8      call  0x140739200
|      ||   0x14603e3dd      add   qword [rdi+0x90], 0x20              ; [0x20:8]=-1 ; 32
|     ,===< 0x14603e3e5      jmp   0x14603e3f9
|     ||`-> 0x14603e3e7      mov   r8, rbp
|     ||    0x14603e3ea      mov   rdx, rax
|     ||    0x14603e3ed      lea   rcx, qword [rdi+0x88]
|     ||    0x14603e3f4      call  0x140860160
|     ||    ; CODE XREF from fcn.14603e370 @ 0x14603e3e5
|     `---> 0x14603e3f9      cmp   byte [0x14d920570], 0x00            ; [0x14d920570:1]=0
|      |,=< 0x14603e400      jz    0x14603e4a9
|      ||   0x14603e406      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|      ||   0x14603e40d      cmp   al, 0x06                            ; 6
|     ,===< 0x14603e40f      jb    0x14603e4a9
|     |||   0x14603e415      shr   rax, 0x08
|     |||   0x14603e419      cmp   al, 0x05                            ; 5
|    ,====< 0x14603e41b      jb    0x14603e4a9
|    ||||   0x14603e421      mov   rax, qword [rdi+0x90]
|    ||||   0x14603e428      sub   rax, qword [rdi+0x88]
|    ||||   0x14603e42f      sar   rax, 0x05
|    ||||   0x14603e433      lea   rcx, qword [0x148f72af0]            ; "[DFLog::RbxTransportClientLog] Message queued (channel not yet open), queueSize={}"
|    ||||   0x14603e43a      mov   qword [var_68h], rcx
|    ||||   0x14603e43f      mov   qword [var_60h], 0x52               ; 'R'
|    ||||                                                              ; [0x52:8]=-1 ; 82
|    ||||   0x14603e448      mov   qword [var_78h], rax
|    ||||   0x14603e44d      movaps xmm0, xmmword [var_78h]
|    ||||   0x14603e452      movdqa xmmword [var_38h], xmm0
|    ||||   0x14603e458      mov   qword [var_78h], 0x04
|    ||||   0x14603e461      lea   rax, qword [var_38h]
|    ||||   0x14603e466      mov   qword [var_70h], rax
|    ||||   0x14603e46b      movaps xmm0, xmmword [var_78h]
|    ||||   0x14603e470      movdqa xmmword [var_78h], xmm0
|    ||||   0x14603e476      movaps xmm1, xmmword [var_68h]
|    ||||   0x14603e47b      movdqa xmmword [var_68h], xmm1
|    ||||   0x14603e481      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|    ||||   0x14603e488      movaps xmmword [var_48h], xmm0
|    ||||   0x14603e48d      mov   byte [var_88h], 0x01
|    ||||   0x14603e492      lea   r9, qword [var_78h]
|    ||||   0x14603e497      lea   r8, qword [var_68h]
|    ||||   0x14603e49c      mov   dl, 0x05
|    ||||   0x14603e49e      lea   rcx, qword [var_48h]
|    ||||   0x14603e4a3      call  0x1438602b0
|    ||||   0x14603e4a8      nop
|    ||||   ; CODE XREF from fcn.14603e370 @ 0x14603e3bd
|    ````-> 0x14603e4a9      mov   rcx, rbx
|           0x14603e4ac      call  fcn.1427c8240
|           0x14603e4b1      nop
|           0x14603e4b2      mov   rcx, qword [var_28h]
|           0x14603e4ba      xor   rcx, rsp
|           0x14603e4bd      call  0x14730fca0
|           0x14603e4c2      mov   rbx, qword [var_18h]
|           0x14603e4ca      add   rsp, 0x90
|           0x14603e4d1      pop   rdi
|           0x14603e4d2      pop   rsi
|           0x14603e4d3      pop   rbp
\           0x14603e4d4      ret
