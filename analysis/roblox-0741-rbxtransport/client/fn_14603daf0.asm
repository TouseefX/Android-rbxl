/ fcn.14603daf0(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_308h @ stack - 0x308
|           ; var int64_t var_2f8h @ stack - 0x2f8
|           ; var int64_t var_2f0h @ stack - 0x2f0
|           ; var int64_t var_2e8h @ stack - 0x2e8
|           ; var int64_t var_2e0h @ stack - 0x2e0
|           ; var int64_t var_2d8h @ stack - 0x2d8
|           ; var int64_t var_2c8h @ stack - 0x2c8
|           ; var int64_t var_2b8h @ stack - 0x2b8
|           ; var int64_t var_2a8h @ stack - 0x2a8
|           ; var int64_t var_2a0h @ stack - 0x2a0
|           ; var int64_t var_298h @ stack - 0x298
|           ; var int64_t var_290h @ stack - 0x290
|           ; var int64_t var_288h @ stack - 0x288
|           ; var int64_t var_280h @ stack - 0x280
|           ; var int64_t var_278h @ stack - 0x278
|           ; var int64_t var_270h @ stack - 0x270
|           ; var int64_t var_268h @ stack - 0x268
|           ; var int64_t var_260h @ stack - 0x260
|           ; var int64_t var_258h @ stack - 0x258
|           ; var int64_t var_250h @ stack - 0x250
|           ; var int64_t var_248h @ stack - 0x248
|           ; var int64_t var_240h @ stack - 0x240
|           ; var int64_t var_238h @ stack - 0x238
|           ; var int64_t var_230h @ stack - 0x230
|           ; var int64_t var_228h @ stack - 0x228
|           ; var int64_t var_220h @ stack - 0x220
|           ; var int64_t var_218h @ stack - 0x218
|           ; var int64_t var_210h @ stack - 0x210
|           ; var int64_t var_208h @ stack - 0x208
|           ; var int64_t var_1f8h @ stack - 0x1f8
|           ; var int64_t var_1e8h @ stack - 0x1e8
|           ; var int64_t var_1d0h @ stack - 0x1d0
|           ; var int64_t var_1a8h @ stack - 0x1a8
|           ; var int64_t var_150h @ stack - 0x150
|           ; var int64_t var_148h @ stack - 0x148
|           ; var int64_t var_140h @ stack - 0x140
|           ; var int64_t var_138h @ stack - 0x138
|           ; var int64_t var_108h @ stack - 0x108
|           ; var int64_t var_f8h @ stack - 0xf8
|           ; var int64_t var_e8h @ stack - 0xe8
|           ; var int64_t var_d0h @ stack - 0xd0
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_50h @ stack - 0x50
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           0x14603daf0      mov   qword [var_10h], rbx
|           0x14603daf5      mov   qword [var_18h], rsi
|           0x14603dafa      push  rbp
|           0x14603dafb      push  rdi
|           0x14603dafc      push  r12
|           0x14603dafe      push  r14
|           0x14603db00      push  r15
|           0x14603db02      lea   rbp, qword [var_228h]
|           0x14603db0a      sub   rsp, 0x300
|           0x14603db11      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14603db18      xor   rax, rsp
|           0x14603db1b      mov   qword [var_38h], rax
|           0x14603db22      mov   rdi, rcx                            ; arg1
|           0x14603db25      xorps xmm0, xmm0
|           0x14603db28      movdqu xmmword [var_2f8h], xmm0
|           0x14603db2e      xor   r15d, r15d
|           0x14603db31      mov   qword [var_2e8h], r15
|           0x14603db36      mov   qword [var_2e0h], r15
|           0x14603db3b      mov   qword [var_2d8h], r15
|           0x14603db40      lea   ecx, qword [r15+0x10]
|           0x14603db44      call  0x14385bce0
|           0x14603db49      mov   qword [rax+0x08], r15
|           0x14603db4d      mov   qword [var_2f8h], rax
|           0x14603db52      lea   rcx, qword [var_2f8h]
|           0x14603db57      mov   qword [rax], rcx
|           0x14603db5a      lea   rbx, qword [rdi+0x50]
|           0x14603db5e      mov   qword [var_2a8h], rbx
|           0x14603db62      mov   rcx, rbx
|           0x14603db65      call  0x1427c8210
|           0x14603db6a      nop
|           0x14603db6b      mov   rcx, qword [rdi+0x58]
|           0x14603db6f      mov   rax, qword [rcx]
|           0x14603db72      lea   rdx, qword [var_138h]
|           0x14603db79      call  qword [rax+0x10]                    ; 16
|           0x14603db7c      mov   r14, rax
|           0x14603db7f      lea   rax, qword [var_2f8h]
|           0x14603db84      cmp   rax, r14
|       ,=< 0x14603db87      jz    0x14603dcd0
|       |   0x14603db8d      mov   rdx, qword [var_2d8h]
|       |   0x14603db92      test  rdx, rdx
|      ,==< 0x14603db95      jz    0x14603dbf4
|      ||   0x14603db97      nop   word [rax+rax*1], ax
|     .---> 0x14603dba0      mov   rcx, qword [var_2e0h]
|     :||   0x14603dba5      dec   rcx
|     :||   0x14603dba8      add   rdx, rcx
|     :||   0x14603dbab      mov   rcx, qword [var_2e8h]
|     :||   0x14603dbb0      dec   rcx
|     :||   0x14603dbb3      and   rcx, rdx
|     :||   0x14603dbb6      mov   rax, qword [var_2f0h]
|     :||   0x14603dbbb      mov   rsi, qword [rax+rcx*8]
|     :||   0x14603dbbf      lea   rcx, qword [rsi+0x40]
|     :||   0x14603dbc3      cmp   byte [rcx+0x58], 0x00
|    ,====< 0x14603dbc7      jz    0x14603dbce
|    |:||   0x14603dbc9      call  0x1435d0810
|    `----> 0x14603dbce      lea   rcx, qword [rsi+0x18]
|     :||   0x14603dbd2      call  0x14073a8e0
|     :||   0x14603dbd7      mov   rcx, qword [rsi]
|     :||   0x14603dbda      call  0x14385bd60
|     :||   0x14603dbdf      mov   rdx, qword [var_2d8h]
|     :||   0x14603dbe4      sub   rdx, 0x01
|     :||   0x14603dbe8      mov   qword [var_2d8h], rdx
|     `===< 0x14603dbed      jnz   0x14603dba0
|      ||   0x14603dbef      mov   qword [var_2e0h], r15
|      `--> 0x14603dbf4      mov   rsi, qword [var_2e8h]
|       |   0x14603dbf9      mov   rcx, qword [var_2f0h]
|       |   0x14603dbfe      test  rsi, rsi
|      ,==< 0x14603dc01      jz    0x14603dc2b
|     .---> 0x14603dc03      dec   rsi
|     :||   0x14603dc06      mov   rax, qword [rcx+rsi*8]
|     :||   0x14603dc0a      test  rax, rax
|    ,====< 0x14603dc0d      jz    0x14603dc21
|    |:||   0x14603dc0f      mov   edx, 0xa8                           ; 168
|    |:||   0x14603dc14      mov   rcx, rax
|    |:||   0x14603dc17      call  0x14385bd60
|    |:||   0x14603dc1c      mov   rcx, qword [var_2f0h]
|    `----> 0x14603dc21      test  rsi, rsi
|     `===< 0x14603dc24      jnz   0x14603dc03
|      ||   0x14603dc26      mov   rsi, qword [var_2e8h]
|      `--> 0x14603dc2b      test  rcx, rcx
|      ,==< 0x14603dc2e      jz    0x14603dc65
|      ||   0x14603dc30      lea   rdx, qword [rsi*8]
|      ||   0x14603dc38      mov   rax, rcx
|      ||   0x14603dc3b      cmp   rdx, 0x1000
|     ,===< 0x14603dc42      jb    0x14603dc60
|     |||   0x14603dc44      add   rdx, 0x27                           ; 39
|     |||   0x14603dc48      mov   rcx, qword [rcx-0x08]
|     |||   0x14603dc4c      sub   rax, rcx
|     |||   0x14603dc4f      add   rax, 0xfffffffffffffff8
|     |||   0x14603dc53      cmp   rax, 0x1f                           ; 31
|    ,====< 0x14603dc57      jbe   0x14603dc60
|    ||||   0x14603dc59      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|    ||||   0x14603dc5f      int3
|    ``---> 0x14603dc60      call  0x14385bd60
|      `--> 0x14603dc65      mov   qword [var_2e8h], r15
|       |   0x14603dc6a      mov   qword [var_2f0h], r15
|       |   0x14603dc6f      mov   rcx, qword [var_2f8h]
|       |   0x14603dc74      mov   rax, qword [r14]
|       |   0x14603dc77      mov   qword [var_2f8h], rax
|       |   0x14603dc7c      mov   qword [r14], rcx
|       |   0x14603dc7f      mov   rax, qword [var_2f8h]
|       |   0x14603dc84      test  rax, rax
|      ,==< 0x14603dc87      jz    0x14603dc94
|      ||   0x14603dc89      lea   rcx, qword [var_2f8h]
|      ||   0x14603dc8e      mov   qword [rax], rcx
|      ||   0x14603dc91      mov   rcx, qword [r14]
|      `--> 0x14603dc94      test  rcx, rcx
|      ,==< 0x14603dc97      jz    0x14603dc9c
|      ||   0x14603dc99      mov   qword [rcx], r14
|      `--> 0x14603dc9c      mov   rax, qword [r14+0x08]
|       |   0x14603dca0      mov   qword [var_2f0h], rax
|       |   0x14603dca5      mov   rax, qword [r14+0x10]
|       |   0x14603dca9      mov   qword [var_2e8h], rax
|       |   0x14603dcae      mov   rax, qword [r14+0x18]
|       |   0x14603dcb2      mov   qword [var_2e0h], rax
|       |   0x14603dcb7      mov   rax, qword [r14+0x20]
|       |   0x14603dcbb      mov   qword [var_2d8h], rax
|       |   0x14603dcc0      mov   qword [r14+0x08], r15
|       |   0x14603dcc4      mov   qword [r14+0x10], r15
|       |   0x14603dcc8      mov   qword [r14+0x18], r15
|       |   0x14603dccc      mov   qword [r14+0x20], r15
|       `-> 0x14603dcd0      lea   rcx, qword [var_138h]
|           0x14603dcd7      call  0x1435dc900
|           0x14603dcdc      mov   rcx, qword [var_138h]
|           0x14603dce3      mov   qword [var_138h], r15
|           0x14603dcea      mov   edx, 0x10                           ; 16
|           0x14603dcef      call  0x14385bd60
|           0x14603dcf4      nop
|           0x14603dcf5      mov   rcx, rbx
|           0x14603dcf8      call  0x1427c8240
|           0x14603dcfd      nop
|           0x14603dcfe      cmp   qword [var_2d8h], 0x00
|       ,=< 0x14603dd04      jz    0x14603e13a
|       |   0x14603dd0a      lea   r12, qword [0x148f72bf0]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient Unhandled event type: {}"
|       |   0x14603dd11      lea   rsi, qword [0x148f72ba0]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient ACK ReceiveChannelOpened {}"
|       |   0x14603dd18      lea   r14, qword [0x1435e0c40]
|      .--> 0x14603dd1f      mov   rdx, qword [var_2e8h]
|      :|   0x14603dd24      dec   rdx
|      :|   0x14603dd27      and   rdx, qword [var_2e0h]
|      :|   0x14603dd2c      mov   rax, qword [var_2f0h]
|      :|   0x14603dd31      mov   rdx, qword [rax+rdx*8]
|      :|   0x14603dd35      lea   rcx, qword [var_e8h]
|      :|   0x14603dd3c      call  0x1435e2050
|      :|   0x14603dd41      nop
|      :|   0x14603dd42      mov   rcx, qword [var_2e8h]
|      :|   0x14603dd47      dec   rcx
|      :|   0x14603dd4a      and   rcx, qword [var_2e0h]
|      :|   0x14603dd4f      mov   rax, qword [var_2f0h]
|      :|   0x14603dd54      mov   rbx, qword [rax+rcx*8]
|      :|   0x14603dd58      lea   rcx, qword [rbx+0x40]
|      :|   0x14603dd5c      cmp   byte [rcx+0x58], 0x00
|     ,===< 0x14603dd60      jz    0x14603dd67
|     |:|   0x14603dd62      call  0x1435d0810
|     `---> 0x14603dd67      lea   rcx, qword [rbx+0x18]
|      :|   0x14603dd6b      call  0x14073a8e0
|      :|   0x14603dd70      mov   rcx, qword [rbx]
|      :|   0x14603dd73      call  0x14385bd60
|      :|   0x14603dd78      sub   qword [var_2d8h], 0x01
|     ,===< 0x14603dd7e      jnz   0x14603dd87
|     |:|   0x14603dd80      mov   qword [var_2e0h], r15
|    ,====< 0x14603dd85      jmp   0x14603dd8c
|    |`---> 0x14603dd87      inc   qword [var_2e0h]
|    | :|   ; CODE XREF from fcn.14603daf0 @ 0x14603dd85
|    `----> 0x14603dd8c      lea   rdx, qword [var_e8h]
|      :|   0x14603dd93      lea   rcx, qword [var_1e8h]
|      :|   0x14603dd97      call  0x1435e2050
|      :|   0x14603dd9c      mov   byte [var_140h], 0x01
|      :|   0x14603dda3      cmp   byte [var_50h], 0x00
|     ,===< 0x14603ddaa      jz    0x14603ddb8
|     |:|   0x14603ddac      lea   rcx, qword [var_a8h]
|     |:|   0x14603ddb3      call  0x1435d0810
|     `---> 0x14603ddb8      lea   rcx, qword [var_d0h]
|      :|   0x14603ddbf      call  0x14073a8e0
|      :|   0x14603ddc4      mov   rcx, qword [var_e8h]
|      :|   0x14603ddcb      call  0x14385bd60
|      :|   0x14603ddd0      nop
|      :|   0x14603ddd1      cmp   byte [var_140h], 0x00
|     ,===< 0x14603ddd8      jnz   0x14603dddf
|    ,====< 0x14603ddda      jmp   0x14603e12e
|    |`---> 0x14603dddf      movzx edx, byte [var_148h]
|    | :|   0x14603dde6      mov   ecx, edx
|    | :|   0x14603dde8      cmp   byte [0x14d9205c0], 0x00            ; [0x14d9205c0:1]=0
|    |,===< 0x14603ddef      jz    0x14603df87
|    ||:|   0x14603ddf5      sub   ecx, 0x01
|   ,=====< 0x14603ddf8      jz    0x14603df71
|   |||:|   0x14603ddfe      sub   ecx, 0x01
|  ,======< 0x14603de01      jz    0x14603ded2
|  ||||:|   0x14603de07      sub   ecx, 0x01
| ,=======< 0x14603de0a      jz    0x14603debc
| |||||:|   0x14603de10      cmp   ecx, 0x04                           ; 4
| ========< 0x14603de13      jz    0x14603dea6
| |||||:|   0x14603de19      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
| |||||:|   0x14603de20      cmp   al, 0x06                            ; 6
| ========< 0x14603de22      jb    0x14603e0fe
| |||||:|   0x14603de28      shr   rax, 0x08
| |||||:|   0x14603de2c      cmp   al, 0x06                            ; 6
| ========< 0x14603de2e      jb    0x14603e0fe
| |||||:|   0x14603de34      mov   qword [var_298h], r12
| |||||:|   0x14603de38      mov   qword [var_290h], 0x4a              ; 'J' ; 74
| |||||:|   0x14603de40      mov   dword [var_208h], edx
| |||||:|   0x14603de43      movaps xmm0, xmmword [var_208h]
| |||||:|   0x14603de47      movdqa xmmword [var_108h], xmm0
| |||||:|   0x14603de4f      mov   qword [var_2a8h], 0x01
| |||||:|   0x14603de57      lea   rax, qword [var_108h]
| |||||:|   0x14603de5e      mov   qword [var_2a0h], rax
| |||||:|   0x14603de62      movaps xmm0, xmmword [var_2a8h]
| |||||:|   0x14603de66      movdqa xmmword [var_2c8h], xmm0
| |||||:|   0x14603de6c      movaps xmm1, xmmword [var_298h]
| |||||:|   0x14603de70      movdqa xmmword [var_2b8h], xmm1
| |||||:|   0x14603de76      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| |||||:|   0x14603de7d      movaps xmmword [var_f8h], xmm0
| |||||:|   0x14603de84      mov   byte [var_308h], 0x01
| |||||:|   0x14603de89      lea   r9, qword [var_2c8h]
| |||||:|   0x14603de8e      lea   r8, qword [var_2b8h]
| |||||:|   0x14603de93      mov   dl, 0x06
| |||||:|   0x14603de95      lea   rcx, qword [var_f8h]
| |||||:|   0x14603de9c      call  0x1438602b0
| ========< 0x14603dea1      jmp   0x14603e0fe
| --------> 0x14603dea6      mov   rax, qword [rdi]
| |||||:|   0x14603dea9      lea   r8, qword [var_1d0h]
| |||||:|   0x14603dead      mov   rdx, qword [rdi+0x58]
| |||||:|   0x14603deb1      mov   rcx, rdi
| |||||:|   0x14603deb4      call  qword [rax+0x28]                    ; 40
| ========< 0x14603deb7      jmp   0x14603e0fe
| `-------> 0x14603debc      mov   rax, qword [rdi]
|  ||||:|   0x14603debf      lea   r8, qword [var_1e8h]
|  ||||:|   0x14603dec3      mov   rdx, qword [rdi+0x58]
|  ||||:|   0x14603dec7      mov   rcx, rdi
|  ||||:|   0x14603deca      call  qword [rax+0x20]                    ; 32
| ,=======< 0x14603decd      jmp   0x14603e0fe
| |`------> 0x14603ded2      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
| | |||:|   0x14603ded9      cmp   al, 0x06                            ; 6
| |,======< 0x14603dedb      jb    0x14603df5b
| |||||:|   0x14603dedd      shr   rax, 0x08
| |||||:|   0x14603dee1      cmp   al, 0x04                            ; 4
| ========< 0x14603dee3      jb    0x14603df5b
| |||||:|   0x14603dee5      mov   qword [var_268h], rsi
| |||||:|   0x14603dee9      mov   qword [var_260h], 0x4d              ; 'M' ; 77
| |||||:|   0x14603def1      lea   rax, qword [var_1d0h]
| |||||:|   0x14603def5      mov   qword [var_288h], rax
| |||||:|   0x14603def9      mov   qword [var_280h], r14
| |||||:|   0x14603defd      movaps xmm0, xmmword [var_288h]
| |||||:|   0x14603df01      movdqa xmmword [var_f8h], xmm0
| |||||:|   0x14603df09      mov   qword [var_278h], 0x0f              ; 0xf ; 15
| |||||:|   0x14603df11      lea   rax, qword [var_f8h]
| |||||:|   0x14603df18      mov   qword [var_270h], rax
| |||||:|   0x14603df1c      movaps xmm0, xmmword [var_278h]
| |||||:|   0x14603df20      movdqa xmmword [var_2b8h], xmm0
| |||||:|   0x14603df26      movaps xmm1, xmmword [var_268h]
| |||||:|   0x14603df2a      movdqa xmmword [var_2c8h], xmm1
| |||||:|   0x14603df30      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| |||||:|   0x14603df37      movaps xmmword [var_108h], xmm0
| |||||:|   0x14603df3e      mov   byte [var_308h], 0x01
| |||||:|   0x14603df43      lea   r9, qword [var_2b8h]
| |||||:|   0x14603df48      lea   r8, qword [var_2c8h]
| |||||:|   0x14603df4d      mov   dl, 0x04
| |||||:|   0x14603df4f      lea   rcx, qword [var_108h]
| |||||:|   0x14603df56      call  0x1438602b0
| -`------> 0x14603df5b      mov   rax, qword [rdi]
| | |||:|   0x14603df5e      lea   r8, qword [var_1e8h]
| | |||:|   0x14603df62      mov   rdx, qword [rdi+0x58]
| | |||:|   0x14603df66      mov   rcx, rdi
| | |||:|   0x14603df69      call  qword [rax+0x38]                    ; 56
| |,======< 0x14603df6c      jmp   0x14603e0fe
| ||`-----> 0x14603df71      mov   rax, qword [rdi]
| || ||:|   0x14603df74      lea   r8, qword [var_1e8h]
| || ||:|   0x14603df78      mov   rdx, qword [rdi+0x58]
| || ||:|   0x14603df7c      mov   rcx, rdi
| || ||:|   0x14603df7f      call  qword [rax+0x40]                    ; 64
| ||,=====< 0x14603df82      jmp   0x14603e0fe
| ||||`---> 0x14603df87      sub   ecx, 0x01
| ||||,===< 0x14603df8a      jz    0x14603e0f0
| |||||:|   0x14603df90      sub   ecx, 0x01
| ========< 0x14603df93      jz    0x14603e05c
| |||||:|   0x14603df99      sub   ecx, 0x01
| ========< 0x14603df9c      jz    0x14603e04a
| |||||:|   0x14603dfa2      cmp   ecx, 0x04                           ; 4
| ========< 0x14603dfa5      jz    0x14603e038
| |||||:|   0x14603dfab      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
| |||||:|   0x14603dfb2      cmp   al, 0x06                            ; 6
| ========< 0x14603dfb4      jb    0x14603e0fe
| |||||:|   0x14603dfba      shr   rax, 0x08
| |||||:|   0x14603dfbe      cmp   al, 0x06                            ; 6
| ========< 0x14603dfc0      jb    0x14603e0fe
| |||||:|   0x14603dfc6      mov   qword [var_248h], r12
| |||||:|   0x14603dfca      mov   qword [var_240h], 0x4a              ; 'J' ; 74
| |||||:|   0x14603dfd2      mov   dword [var_1f8h], edx
| |||||:|   0x14603dfd5      movaps xmm0, xmmword [var_1f8h]
| |||||:|   0x14603dfd9      movdqa xmmword [var_f8h], xmm0
| |||||:|   0x14603dfe1      mov   qword [var_258h], 0x01
| |||||:|   0x14603dfe9      lea   rax, qword [var_f8h]
| |||||:|   0x14603dff0      mov   qword [var_250h], rax
| |||||:|   0x14603dff4      movaps xmm0, xmmword [var_258h]
| |||||:|   0x14603dff8      movdqa xmmword [var_2b8h], xmm0
| |||||:|   0x14603dffe      movaps xmm1, xmmword [var_248h]
| |||||:|   0x14603e002      movdqa xmmword [var_2c8h], xmm1
| |||||:|   0x14603e008      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| |||||:|   0x14603e00f      movaps xmmword [var_108h], xmm0
| |||||:|   0x14603e016      mov   byte [var_308h], 0x01
| |||||:|   0x14603e01b      lea   r9, qword [var_2b8h]
| |||||:|   0x14603e020      lea   r8, qword [var_2c8h]
| |||||:|   0x14603e025      mov   dl, 0x06
| |||||:|   0x14603e027      lea   rcx, qword [var_108h]
| |||||:|   0x14603e02e      call  0x1438602b0
| ========< 0x14603e033      jmp   0x14603e0fe
| --------> 0x14603e038      mov   rax, qword [rdi]
| |||||:|   0x14603e03b      lea   rdx, qword [var_1d0h]
| |||||:|   0x14603e03f      mov   rcx, rdi
| |||||:|   0x14603e042      call  qword [rax+0x48]                    ; 72
| ========< 0x14603e045      jmp   0x14603e0fe
| --------> 0x14603e04a      mov   rax, qword [rdi]
| |||||:|   0x14603e04d      lea   rdx, qword [var_1e8h]
| |||||:|   0x14603e051      mov   rcx, rdi
| |||||:|   0x14603e054      call  qword [rax+0x50]                    ; 80
| ========< 0x14603e057      jmp   0x14603e0fe
| --------> 0x14603e05c      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
| |||||:|   0x14603e063      cmp   al, 0x06                            ; 6
| ========< 0x14603e065      jb    0x14603e0e5
| |||||:|   0x14603e067      shr   rax, 0x08
| |||||:|   0x14603e06b      cmp   al, 0x04                            ; 4
| ========< 0x14603e06d      jb    0x14603e0e5
| |||||:|   0x14603e06f      mov   qword [var_218h], rsi
| |||||:|   0x14603e073      mov   qword [var_210h], 0x4d              ; 'M' ; 77
| |||||:|   0x14603e07b      lea   rax, qword [var_1d0h]
| |||||:|   0x14603e07f      mov   qword [var_238h], rax
| |||||:|   0x14603e083      mov   qword [var_230h], r14
| |||||:|   0x14603e087      movaps xmm0, xmmword [var_238h]
| |||||:|   0x14603e08b      movdqa xmmword [var_f8h], xmm0
| |||||:|   0x14603e093      mov   qword [rbp], 0x0f                   ; 0xf ; 15
| |||||:|   0x14603e09b      lea   rax, qword [var_f8h]
| |||||:|   0x14603e0a2      mov   qword [var_220h], rax
| |||||:|   0x14603e0a6      movaps xmm0, xmmword [rbp]
| |||||:|   0x14603e0aa      movdqa xmmword [var_2b8h], xmm0
| |||||:|   0x14603e0b0      movaps xmm1, xmmword [var_218h]
| |||||:|   0x14603e0b4      movdqa xmmword [var_2c8h], xmm1
| |||||:|   0x14603e0ba      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| |||||:|   0x14603e0c1      movaps xmmword [var_108h], xmm0
| |||||:|   0x14603e0c8      mov   byte [var_308h], 0x01
| |||||:|   0x14603e0cd      lea   r9, qword [var_2b8h]
| |||||:|   0x14603e0d2      lea   r8, qword [var_2c8h]
| |||||:|   0x14603e0d7      mov   dl, 0x04
| |||||:|   0x14603e0d9      lea   rcx, qword [var_108h]
| |||||:|   0x14603e0e0      call  0x1438602b0
| --------> 0x14603e0e5      mov   rax, qword [rdi]
| |||||:|   0x14603e0e8      mov   rcx, rdi
| |||||:|   0x14603e0eb      call  qword [rax+0x60]                    ; 96
| ========< 0x14603e0ee      jmp   0x14603e0fe
| ||||`---> 0x14603e0f0      mov   rax, qword [rdi]
| |||| :|   0x14603e0f3      lea   rdx, qword [var_1e8h]
| |||| :|   0x14603e0f7      mov   rcx, rdi
| |||| :|   0x14603e0fa      call  qword [rax+0x58]                    ; 88
| |||| :|   0x14603e0fd      nop
| |||| :|   ; XREFS: CODE 0x14603dea1  CODE 0x14603deb7  CODE 0x14603decd
| |||| :|   ; XREFS: CODE 0x14603df6c  CODE 0x14603df82  CODE 0x14603e033
| |||| :|   ; XREFS: CODE 0x14603e045  CODE 0x14603e057  CODE 0x14603e0ee
| ```-----> 0x14603e0fe      cmp   byte [var_140h], 0x00
|    |,===< 0x14603e105      jz    0x14603e12e
|    ||:|   0x14603e107      cmp   byte [var_150h], 0x00
|   ,=====< 0x14603e10e      jz    0x14603e11c
|   |||:|   0x14603e110      lea   rcx, qword [var_1a8h]
|   |||:|   0x14603e117      call  0x1435d0810
|   `-----> 0x14603e11c      lea   rcx, qword [var_1d0h]
|    ||:|   0x14603e120      call  0x14073a8e0
|    ||:|   0x14603e125      mov   rcx, qword [var_1e8h]
|    ||:|   0x14603e129      call  0x14385bd60
|    ||:|   ; CODE XREF from fcn.14603daf0 @ 0x14603ddda
|    ``---> 0x14603e12e      cmp   qword [var_2d8h], 0x00
|      `==< 0x14603e134      jnz   0x14603dd1f
|       `-> 0x14603e13a      cmp   qword [var_2d8h], 0x00
|       ,=< 0x14603e140      jz    0x14603e1b0
|       |   0x14603e142      mov   rcx, qword [var_2e0h]
|       |   0x14603e147      nop   word [rax+rax*1], ax
|      .--> 0x14603e150      mov   rax, qword [var_2d8h]
|      :|   0x14603e155      dec   rax
|      :|   0x14603e158      add   rcx, rax
|      :|   0x14603e15b      mov   rax, qword [var_2e8h]
|      :|   0x14603e160      dec   rax
|      :|   0x14603e163      and   rcx, rax
|      :|   0x14603e166      mov   rax, qword [var_2f0h]
|      :|   0x14603e16b      mov   rbx, qword [rax+rcx*8]
|      :|   0x14603e16f      lea   rcx, qword [rbx+0x40]
|      :|   0x14603e173      cmp   byte [rcx+0x58], 0x00
|     ,===< 0x14603e177      jz    0x14603e17e
|     |:|   0x14603e179      call  0x1435d0810
|     `---> 0x14603e17e      lea   rcx, qword [rbx+0x18]
|      :|   0x14603e182      call  0x14073a8e0
|      :|   0x14603e187      mov   rcx, qword [rbx]
|      :|   0x14603e18a      call  0x14385bd60
|      :|   0x14603e18f      mov   rax, qword [var_2d8h]
|      :|   0x14603e194      sub   rax, 0x01
|      :|   0x14603e198      mov   qword [var_2d8h], rax
|      :|   0x14603e19d      mov   rcx, qword [var_2e0h]
|      :|   0x14603e1a2      cmovz rcx, r15
|      :|   0x14603e1a6      mov   qword [var_2e0h], rcx
|      :|   0x14603e1ab      test  rax, rax
|      `==< 0x14603e1ae      jnz   0x14603e150
|       `-> 0x14603e1b0      mov   rbx, qword [var_2e8h]
|           0x14603e1b5      mov   rcx, qword [var_2f0h]
|           0x14603e1ba      test  rbx, rbx
|       ,=< 0x14603e1bd      jz    0x14603e1e8
|       |   0x14603e1bf      nop
|      .--> 0x14603e1c0      dec   rbx
|      :|   0x14603e1c3      mov   rax, qword [rcx+rbx*8]
|      :|   0x14603e1c7      test  rax, rax
|     ,===< 0x14603e1ca      jz    0x14603e1de
|     |:|   0x14603e1cc      mov   edx, 0xa8                           ; 168
|     |:|   0x14603e1d1      mov   rcx, rax
|     |:|   0x14603e1d4      call  0x14385bd60
|     |:|   0x14603e1d9      mov   rcx, qword [var_2f0h]
|     `---> 0x14603e1de      test  rbx, rbx
|      `==< 0x14603e1e1      jnz   0x14603e1c0
|       |   0x14603e1e3      mov   rbx, qword [var_2e8h]
|       `-> 0x14603e1e8      test  rcx, rcx
|       ,=< 0x14603e1eb      jz    0x14603e222
|       |   0x14603e1ed      lea   rdx, qword [rbx*8]
|       |   0x14603e1f5      mov   rax, rcx
|       |   0x14603e1f8      cmp   rdx, 0x1000
|      ,==< 0x14603e1ff      jb    0x14603e21d
|      ||   0x14603e201      add   rdx, 0x27                           ; 39
|      ||   0x14603e205      mov   rcx, qword [rcx-0x08]
|      ||   0x14603e209      sub   rax, rcx
|      ||   0x14603e20c      add   rax, 0xfffffffffffffff8
|      ||   0x14603e210      cmp   rax, 0x1f                           ; 31
|     ,===< 0x14603e214      jbe   0x14603e21d
|     |||   0x14603e216      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|     |||   0x14603e21c      int3
|     ``--> 0x14603e21d      call  0x14385bd60
|       `-> 0x14603e222      mov   qword [var_2e8h], r15
|           0x14603e227      mov   qword [var_2f0h], r15
|           0x14603e22c      mov   rcx, qword [var_2f8h]
|           0x14603e231      mov   qword [var_2f8h], r15
|           0x14603e236      mov   edx, 0x10                           ; 16
|           0x14603e23b      call  0x14385bd60
|           0x14603e240      mov   rcx, qword [var_38h]
|           0x14603e247      xor   rcx, rsp
|           0x14603e24a      call  0x14730fca0
|           0x14603e24f      lea   r11, qword [var_28h]
|           0x14603e257      mov   rbx, qword [r11+0x38]
|           0x14603e25b      mov   rsi, qword [r11+0x40]
|           0x14603e25f      mov   rsp, r11
|           0x14603e262      pop   r15
|           0x14603e264      pop   r14
|           0x14603e266      pop   r12
|           0x14603e268      pop   rdi
|           0x14603e269      pop   rbp
\           0x14603e26a      ret
