/ fcn.14603bb70(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_348h @ stack - 0x348
|           ; var int64_t var_338h @ stack - 0x338
|           ; var int64_t var_328h @ stack - 0x328
|           ; var int64_t var_320h @ stack - 0x320
|           ; var int64_t var_318h @ stack - 0x318
|           ; var int64_t var_310h @ stack - 0x310
|           ; var int64_t var_308h @ stack - 0x308
|           ; var int64_t var_300h @ stack - 0x300
|           ; var int64_t var_2f8h @ stack - 0x2f8
|           ; var int64_t var_2f0h @ stack - 0x2f0
|           ; var int64_t var_2e8h @ stack - 0x2e8
|           ; var int64_t var_2e0h @ stack - 0x2e0
|           ; var int64_t var_2d8h @ stack - 0x2d8
|           ; var int64_t var_2d0h @ stack - 0x2d0
|           ; var int64_t var_2c8h @ stack - 0x2c8
|           ; var int64_t var_2c0h @ stack - 0x2c0
|           ; var int64_t var_2b8h @ stack - 0x2b8
|           ; var int64_t var_2b0h @ stack - 0x2b0
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
|           ; var int64_t var_208h @ stack - 0x208
|           ; var int64_t var_1f8h @ stack - 0x1f8
|           ; var int64_t var_1e8h @ stack - 0x1e8
|           ; var int64_t var_1d8h @ stack - 0x1d8
|           ; var int64_t var_1c8h @ stack - 0x1c8
|           ; var int64_t var_1b8h @ stack - 0x1b8
|           ; var int64_t var_1a8h @ stack - 0x1a8
|           ; var int64_t var_198h @ stack - 0x198
|           ; var int64_t var_188h @ stack - 0x188
|           ; var int64_t var_178h @ stack - 0x178
|           ; var int64_t var_168h @ stack - 0x168
|           ; var int64_t var_158h @ stack - 0x158
|           ; var int64_t var_148h @ stack - 0x148
|           ; var int64_t var_138h @ stack - 0x138
|           ; var int64_t var_128h @ stack - 0x128
|           ; var int64_t var_118h @ stack - 0x118
|           ; var int64_t var_108h @ stack - 0x108
|           ; var int64_t var_f8h @ stack - 0xf8
|           ; var int64_t var_e8h @ stack - 0xe8
|           ; var int64_t var_d8h @ stack - 0xd8
|           ; var int64_t var_c8h @ stack - 0xc8
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_60h @ stack - 0x60
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           0x14603bb70      mov   qword [var_10h], rbx
|           0x14603bb75      mov   qword [var_18h], rsi
|           0x14603bb7a      push  rbp
|           0x14603bb7b      push  rdi
|           0x14603bb7c      push  r12
|           0x14603bb7e      push  r14
|           0x14603bb80      push  r15
|           0x14603bb82      lea   rbp, qword [var_268h]
|           0x14603bb8a      sub   rsp, 0x340
|           0x14603bb91      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14603bb98      xor   rax, rsp
|           0x14603bb9b      mov   qword [var_38h], rax
|           0x14603bba2      mov   rsi, rcx                            ; arg1
|           0x14603bba5      movzx eax, byte [rcx+0x20]                ; arg1
|           0x14603bba9      nop
|           0x14603bbaa      test  al, al
|       ,=< 0x14603bbac      jz    0x14603bc32
|       |   0x14603bbb2      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|       |   0x14603bbb9      cmp   al, 0x06                            ; 6
|      ,==< 0x14603bbbb      jb    0x14603bc2b
|      ||   0x14603bbbd      shr   rax, 0x08
|      ||   0x14603bbc1      cmp   al, 0x02                            ; 2
|     ,===< 0x14603bbc3      jb    0x14603bc2b
|     |||   0x14603bbc5      lea   rax, qword [0x148f72ec0]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient is already connected!"
|     |||   0x14603bbcc      mov   qword [var_328h], rax
|     |||   0x14603bbd1      mov   qword [var_320h], 0x47              ; 'G'
|     |||                                                              ; [0x47:8]=-1 ; 71
|     |||   0x14603bbda      xor   r12d, r12d
|     |||   0x14603bbdd      mov   qword [var_318h], r12
|     |||   0x14603bbe2      lea   rax, qword [var_f8h]
|     |||   0x14603bbe9      mov   qword [var_310h], rax
|     |||   0x14603bbee      movaps xmm0, xmmword [var_318h]
|     |||   0x14603bbf3      movdqa xmmword [var_308h], xmm0
|     |||   0x14603bbf9      movaps xmm1, xmmword [var_328h]
|     |||   0x14603bbfe      movdqa xmmword [var_328h], xmm1
|     |||   0x14603bc04      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|     |||   0x14603bc0b      movaps xmmword [var_318h], xmm0
|     |||   0x14603bc10      mov   byte [var_348h], 0x01
|     |||   0x14603bc15      lea   r9, qword [var_308h]
|     |||   0x14603bc1a      lea   r8, qword [var_328h]
|     |||   0x14603bc1f      mov   dl, 0x02
|     |||   0x14603bc21      lea   rcx, qword [var_318h]
|     |||   0x14603bc26      call  0x1438602b0
|     ``--> 0x14603bc2b      xor   al, al
|      ,==< 0x14603bc2d      jmp   0x14603c1b6
|      |`-> 0x14603bc32      lea   rbx, qword [rcx+0x50]               ; arg1
|      |    0x14603bc36      mov   qword [var_2f0h], rbx
|      |    0x14603bc3b      mov   rcx, rbx
|      |    0x14603bc3e      call  0x1427c8210
|      |    0x14603bc43      nop
|      |    0x14603bc44      mov   rax, qword [rsi+0x60]
|      |    0x14603bc48      xor   r12d, r12d
|      |    0x14603bc4b      test  rax, rax
|      |,=< 0x14603bc4e      jz    0x14603bc56
|      ||   0x14603bc50      mov   rcx, qword [rax+0x18]
|     ,===< 0x14603bc54      jmp   0x14603bc59
|     ||`-> 0x14603bc56      mov   rcx, r12
|     ||    ; CODE XREF from fcn.14603bb70 @ 0x14603bc54
|     `---> 0x14603bc59      mov   r9, qword [rsi+0x68]
|      |    0x14603bc5d      movzx r8d, byte [rsi+0x08]
|      |    0x14603bc62      lea   rdx, qword [var_2f8h]
|      |    0x14603bc67      call  0x1435d0a40
|      |    0x14603bc6c      lea   r14, qword [rsi+0x58]
|      |    0x14603bc70      mov   edi, 0x01
|      |    0x14603bc75      cmp   r14, rax
|      |,=< 0x14603bc78      jz    0x14603bc92
|      ||   0x14603bc7a      mov   rdx, qword [rax]
|      ||   0x14603bc7d      mov   qword [rax], r12
|      ||   0x14603bc80      mov   rcx, qword [r14]
|      ||   0x14603bc83      mov   qword [r14], rdx
|      ||   0x14603bc86      test  rcx, rcx
|     ,===< 0x14603bc89      jz    0x14603bc92
|     |||   0x14603bc8b      mov   rax, qword [rcx]
|     |||   0x14603bc8e      mov   edx, edi
|     |||   0x14603bc90      call  qword [rax]
|     `-`-> 0x14603bc92      mov   rcx, qword [var_2f8h]
|      |    0x14603bc97      test  rcx, rcx
|      |,=< 0x14603bc9a      jz    0x14603bca3
|      ||   0x14603bc9c      mov   rax, qword [rcx]
|      ||   0x14603bc9f      mov   edx, edi
|      ||   0x14603bca1      call  qword [rax]
|      |`-> 0x14603bca3      mov   rcx, qword [r14]
|      |    0x14603bca6      test  rcx, rcx
|      |,=< 0x14603bca9      jnz   0x14603bd51
|      ||   0x14603bcaf      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|      ||   0x14603bcb6      cmp   al, 0x06                            ; 6
|     ,===< 0x14603bcb8      jb    0x14603bd19
|     |||   0x14603bcba      shr   rax, 0x08
|     |||   0x14603bcbe      cmp   al, 0x02                            ; 2
|    ,====< 0x14603bcc0      jb    0x14603bd19
|    ||||   0x14603bcc2      lea   rax, qword [0x148f72f10]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient Failed to create RbxTransport connection!"
|    ||||   0x14603bcc9      mov   qword [var_2d8h], rax
|    ||||   0x14603bccd      mov   qword [var_2d0h], 0x5b              ; '[' ; 91
|    ||||   0x14603bcd5      mov   qword [var_2e8h], r12
|    ||||   0x14603bcd9      lea   rax, qword [var_88h]
|    ||||   0x14603bce0      mov   qword [var_2e0h], rax
|    ||||   0x14603bce4      movaps xmm0, xmmword [var_2e8h]
|    ||||   0x14603bce8      movdqa xmmword [var_218h], xmm0
|    ||||   0x14603bced      movaps xmm1, xmmword [var_2d8h]
|    ||||   0x14603bcf1      movdqa xmmword [var_208h], xmm1
|    ||||   0x14603bcf6      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|    ||||   0x14603bcfd      movaps xmmword [var_1f8h], xmm0
|    ||||   0x14603bd01      mov   byte [var_348h], dil
|    ||||   0x14603bd06      lea   r9, qword [var_218h]
|    ||||   0x14603bd0a      lea   r8, qword [var_208h]
|    ||||   0x14603bd0e      mov   dl, 0x02
|    ||||   0x14603bd10      lea   rcx, qword [var_1f8h]
|    ||||   0x14603bd14      call  0x1438602b0
|    ``---> 0x14603bd19      cmp   byte [0x14d9205e8], r12b            ; [0x14d9205e8:1]=0
|     ,===< 0x14603bd20      jz    0x14603bd41
|     |||   0x14603bd22      xchg  byte [rsi+0x80], dil
|     |||   0x14603bd29      lea   rcx, qword [rsi+0x18]
|     |||   0x14603bd2d      mov   byte [var_338h], r12b
|     |||   0x14603bd32      lea   rdx, qword [var_338h]
|     |||   0x14603bd37      call  0x14097c980
|     |||   0x14603bd3c      mov   rbx, qword [var_2f0h]
|     `---> 0x14603bd41      mov   rcx, rbx
|      ||   0x14603bd44      call  0x1427c8240
|      ||   0x14603bd49      nop
|      ||   0x14603bd4a      xor   al, al
|     ,===< 0x14603bd4c      jmp   0x14603c1b6
|     ||`-> 0x14603bd51      mov   rax, qword [rcx]
|     ||    0x14603bd54      call  qword [rax+0x08]                    ; 8
|     ||    0x14603bd57      nop
|     ||    0x14603bd58      mov   rcx, rbx
|     ||    0x14603bd5b      call  0x1427c8240
|     ||    0x14603bd60      nop
|     ||    0x14603bd61      lea   rdx, qword [var_68h]
|     ||    0x14603bd68      mov   rcx, rsi
|     ||    0x14603bd6b      call  0x14603eb70
|     ||    0x14603bd70      nop
|     ||    0x14603bd71      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|     ||    0x14603bd78      cmp   byte [var_68h], 0x00
|     ||,=< 0x14603bd7f      jnz   0x14603be5a
|     |||   0x14603bd85      cmp   al, 0x06                            ; 6
|    ,====< 0x14603bd87      jb    0x14603be48
|    ||||   0x14603bd8d      shr   rax, 0x08
|    ||||   0x14603bd91      cmp   al, 0x02                            ; 2
|   ,=====< 0x14603bd93      jb    0x14603be48
|   |||||   0x14603bd99      mov   rdx, qword [rsi+0x68]
|   |||||   0x14603bd9d      lea   rcx, qword [0x148f72f70]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient failed to establish connection to server at {}:{}"
|   |||||   0x14603bda4      mov   qword [var_2a8h], rcx
|   |||||   0x14603bda8      mov   qword [var_2a0h], 0x63              ; 'c' ; 99
|   |||||   0x14603bdb0      lea   rcx, qword [rdx+0x08]
|   |||||   0x14603bdb4      cmp   qword [rdx+0x20], 0x10
|  ,======< 0x14603bdb9      jb    0x14603bdbf
|  ||||||   0x14603bdbb      mov   rcx, qword [rdx+0x08]
|  `------> 0x14603bdbf      mov   rax, qword [rdx+0x18]
|   |||||   0x14603bdc3      mov   qword [var_2c8h], rcx
|   |||||   0x14603bdc7      mov   qword [var_2c0h], rax
|   |||||   0x14603bdcb      movzx eax, word [rdx+0x28]
|   |||||   0x14603bdcf      mov   dword [var_1e8h], eax
|   |||||   0x14603bdd5      movups xmm0, xmmword [var_2c8h]
|   |||||   0x14603bdd9      movups xmmword [var_e8h], xmm0
|   |||||   0x14603bde0      movups xmm1, xmmword [var_1e8h]
|   |||||   0x14603bde7      movups xmmword [var_d8h], xmm1
|   |||||   0x14603bdee      mov   qword [var_2b8h], 0x2d              ; '-' ; 45
|   |||||   0x14603bdf6      lea   rax, qword [var_e8h]
|   |||||   0x14603bdfd      mov   qword [var_2b0h], rax
|   |||||   0x14603be01      movaps xmm0, xmmword [var_2b8h]
|   |||||   0x14603be05      movdqa xmmword [var_1d8h], xmm0
|   |||||   0x14603be0d      movaps xmm1, xmmword [var_2a8h]
|   |||||   0x14603be11      movdqa xmmword [var_1c8h], xmm1
|   |||||   0x14603be19      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|   |||||   0x14603be20      movaps xmmword [var_1b8h], xmm0
|   |||||   0x14603be27      mov   byte [var_348h], 0x01
|   |||||   0x14603be2c      lea   r9, qword [var_1d8h]
|   |||||   0x14603be33      lea   r8, qword [var_1c8h]
|   |||||   0x14603be3a      mov   dl, 0x02
|   |||||   0x14603be3c      lea   rcx, qword [var_1b8h]
|   |||||   0x14603be43      call  0x1438602b0
|   ``----> 0x14603be48      cmp   byte [0x14d9205e8], 0x00            ; [0x14d9205e8:1]=0
|    ,====< 0x14603be4f      jz    0x14603bfc4
|   ,=====< 0x14603be55      jmp   0x14603bfaa
|   ||||`-> 0x14603be5a      cmp   al, 0x06                            ; 6
|   ||||,=< 0x14603be5c      jb    0x14603bf1d
|   |||||   0x14603be62      shr   rax, 0x08
|   |||||   0x14603be66      cmp   al, 0x05                            ; 5
|  ,======< 0x14603be68      jb    0x14603bf1d
|  ||||||   0x14603be6e      mov   rdx, qword [rsi+0x68]
|  ||||||   0x14603be72      lea   rcx, qword [0x148f72fe0]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient connection to server at {}:{} established"
|  ||||||   0x14603be79      mov   qword [var_278h], rcx
|  ||||||   0x14603be7d      mov   qword [var_270h], 0x5b              ; '[' ; 91
|  ||||||   0x14603be85      lea   rcx, qword [rdx+0x08]
|  ||||||   0x14603be89      cmp   qword [rdx+0x20], 0x10
| ,=======< 0x14603be8e      jb    0x14603be94
| |||||||   0x14603be90      mov   rcx, qword [rdx+0x08]
| `-------> 0x14603be94      mov   rax, qword [rdx+0x18]
|  ||||||   0x14603be98      mov   qword [var_298h], rcx
|  ||||||   0x14603be9c      mov   qword [var_290h], rax
|  ||||||   0x14603bea0      movzx eax, word [rdx+0x28]
|  ||||||   0x14603bea4      mov   dword [var_1a8h], eax
|  ||||||   0x14603beaa      movups xmm0, xmmword [var_298h]
|  ||||||   0x14603beae      movups xmmword [var_c8h], xmm0
|  ||||||   0x14603beb5      movups xmm1, xmmword [var_1a8h]
|  ||||||   0x14603bebc      movups xmmword [var_b8h], xmm1
|  ||||||   0x14603bec3      mov   qword [var_288h], 0x2d              ; '-' ; 45
|  ||||||   0x14603becb      lea   rax, qword [var_c8h]
|  ||||||   0x14603bed2      mov   qword [var_280h], rax
|  ||||||   0x14603bed6      movaps xmm0, xmmword [var_288h]
|  ||||||   0x14603beda      movdqa xmmword [var_198h], xmm0
|  ||||||   0x14603bee2      movaps xmm1, xmmword [var_278h]
|  ||||||   0x14603bee6      movdqa xmmword [var_188h], xmm1
|  ||||||   0x14603beee      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|  ||||||   0x14603bef5      movaps xmmword [var_178h], xmm0
|  ||||||   0x14603befc      mov   byte [var_348h], 0x01
|  ||||||   0x14603bf01      lea   r9, qword [var_198h]
|  ||||||   0x14603bf08      lea   r8, qword [var_188h]
|  ||||||   0x14603bf0f      mov   dl, 0x05
|  ||||||   0x14603bf11      lea   rcx, qword [var_178h]
|  ||||||   0x14603bf18      call  0x1438602b0
|  `----`-> 0x14603bf1d      mov   rax, qword [rsi]
|   ||||    0x14603bf20      mov   rcx, rsi
|   ||||    0x14603bf23      call  qword [rax+0x68]                    ; 104
|   ||||    0x14603bf26      test  al, al
|   ||||,=< 0x14603bf28      jnz   0x14603bfcb
|   |||||   0x14603bf2e      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|   |||||   0x14603bf35      cmp   al, 0x06                            ; 6
|  ,======< 0x14603bf37      jb    0x14603bfaa
|  ||||||   0x14603bf39      shr   rax, 0x08
|  ||||||   0x14603bf3d      cmp   al, 0x02                            ; 2
| ,=======< 0x14603bf3f      jb    0x14603bfaa
| |||||||   0x14603bf41      lea   rax, qword [0x148f73040]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient failed to open channel!"
| |||||||   0x14603bf48      mov   qword [var_258h], rax
| |||||||   0x14603bf4c      mov   qword [var_250h], 0x49              ; 'I' ; 73
| |||||||   0x14603bf54      mov   qword [rbp], r12
| |||||||   0x14603bf58      lea   rax, qword [var_78h]
| |||||||   0x14603bf5f      mov   qword [var_260h], rax
| |||||||   0x14603bf63      movaps xmm0, xmmword [rbp]
| |||||||   0x14603bf67      movdqa xmmword [var_168h], xmm0
| |||||||   0x14603bf6f      movaps xmm1, xmmword [var_258h]
| |||||||   0x14603bf73      movdqa xmmword [var_158h], xmm1
| |||||||   0x14603bf7b      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| |||||||   0x14603bf82      movaps xmmword [var_148h], xmm0
| |||||||   0x14603bf89      mov   byte [var_348h], 0x01
| |||||||   0x14603bf8e      lea   r9, qword [var_168h]
| |||||||   0x14603bf95      lea   r8, qword [var_158h]
| |||||||   0x14603bf9c      mov   dl, 0x02
| |||||||   0x14603bf9e      lea   rcx, qword [var_148h]
| |||||||   0x14603bfa5      call  0x1438602b0
| |||||||   ; CODE XREF from fcn.14603bb70 @ 0x14603be55
| ```-----> 0x14603bfaa      xchg  byte [rsi+0x80], dil
|    ||||   0x14603bfb1      mov   byte [var_338h], 0x00
|    ||||   0x14603bfb6      lea   rdx, qword [var_338h]
|    ||||   0x14603bfbb      lea   rcx, qword [rsi+0x18]
|    ||||   0x14603bfbf      call  0x14097c980
|    `----> 0x14603bfc4      xor   bl, bl
|    ,====< 0x14603bfc6      jmp   0x14603c1a7
|    |||`-> 0x14603bfcb      mov   eax, edi
|    |||    0x14603bfcd      xchg  byte [rsi+0x20], al
|    |||    0x14603bfd0      mov   rcx, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|    |||    0x14603bfd7      cmp   cl, 0x06                            ; 6
|    |||,=< 0x14603bfda      jb    0x14603c09c
|    ||||   0x14603bfe0      shr   rcx, 0x08
|    ||||   0x14603bfe4      cmp   cl, 0x04                            ; 4
|   ,=====< 0x14603bfe7      jb    0x14603c09c
|   |||||   0x14603bfed      mov   rdx, qword [rsi+0x68]
|   |||||   0x14603bff1      lea   rcx, qword [0x148f73090]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient connected to server at {}:{}"
|   |||||   0x14603bff8      mov   qword [var_228h], rcx
|   |||||   0x14603bffc      mov   qword [var_220h], 0x4e              ; 'N' ; 78
|   |||||   0x14603c004      lea   rcx, qword [rdx+0x08]
|   |||||   0x14603c008      cmp   qword [rdx+0x20], 0x10
|  ,======< 0x14603c00d      jb    0x14603c013
|  ||||||   0x14603c00f      mov   rcx, qword [rdx+0x08]
|  `------> 0x14603c013      mov   rax, qword [rdx+0x18]
|   |||||   0x14603c017      mov   qword [var_248h], rcx
|   |||||   0x14603c01b      mov   qword [var_240h], rax
|   |||||   0x14603c01f      movzx eax, word [rdx+0x28]
|   |||||   0x14603c023      mov   dword [var_138h], eax
|   |||||   0x14603c029      movups xmm0, xmmword [var_248h]
|   |||||   0x14603c02d      movups xmmword [var_a8h], xmm0
|   |||||   0x14603c034      movups xmm1, xmmword [var_138h]
|   |||||   0x14603c03b      movups xmmword [var_98h], xmm1
|   |||||   0x14603c042      mov   qword [var_238h], 0x2d              ; '-' ; 45
|   |||||   0x14603c04a      lea   rax, qword [var_a8h]
|   |||||   0x14603c051      mov   qword [var_230h], rax
|   |||||   0x14603c055      movaps xmm0, xmmword [var_238h]
|   |||||   0x14603c059      movdqa xmmword [var_128h], xmm0
|   |||||   0x14603c061      movaps xmm1, xmmword [var_228h]
|   |||||   0x14603c065      movdqa xmmword [var_118h], xmm1
|   |||||   0x14603c06d      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|   |||||   0x14603c074      movaps xmmword [var_108h], xmm0
|   |||||   0x14603c07b      mov   byte [var_348h], 0x01
|   |||||   0x14603c080      lea   r9, qword [var_128h]
|   |||||   0x14603c087      lea   r8, qword [var_118h]
|   |||||   0x14603c08e      mov   dl, 0x04
|   |||||   0x14603c090      lea   rcx, qword [var_108h]
|   |||||   0x14603c097      call  0x1438602b0
|   `---`-> 0x14603c09c      xchg  byte [rsi+0x09], dil
|    |||    0x14603c0a0      mov   qword [var_318h], rbx
|    |||    0x14603c0a5      mov   rcx, rbx
|    |||    0x14603c0a8      call  0x1427c8210
|    |||    0x14603c0ad      nop
|    |||    0x14603c0ae      mov   byte [rsi+0x82], 0x01
|    |||    0x14603c0b5      lea   r15, qword [rsi+0x88]
|    |||    0x14603c0bc      mov   r14, qword [r15+0x08]
|    |||    0x14603c0c0      mov   rdi, qword [r15]
|    |||    0x14603c0c3      cmp   rdi, r14
|    |||,=< 0x14603c0c6      jz    0x14603c0e4
|    ||||   0x14603c0c8      nop   dword [rax+rax*1], eax
|   .-----> 0x14603c0d0      mov   rdx, rdi
|   :||||   0x14603c0d3      mov   rcx, rsi
|   :||||   0x14603c0d6      call  0x14603e4e0
|   :||||   0x14603c0db      add   rdi, 0x20                           ; 32
|   :||||   0x14603c0df      cmp   rdi, r14
|   `=====< 0x14603c0e2      jnz   0x14603c0d0
|    |||`-> 0x14603c0e4      mov   r8, r15
|    |||    0x14603c0e7      mov   rdx, qword [r15+0x08]
|    |||    0x14603c0eb      mov   rcx, qword [r15]
|    |||    0x14603c0ee      call  0x140734550
|    |||    0x14603c0f3      mov   rax, qword [r15]
|    |||    0x14603c0f6      mov   qword [r15+0x08], rax
|    |||    0x14603c0fa      movzx edi, byte [rsi+0x81]
|    |||    0x14603c101      mov   rcx, rbx
|    |||    0x14603c104      call  0x1427c8240
|    |||    0x14603c109      nop
|    |||    0x14603c10a      test  dil, dil
|    |||,=< 0x14603c10d      jz    0x14603c1a5
|    ||||   0x14603c113      cmp   byte [0x14d920570], 0x00            ; [0x14d920570:1]=0
|   ,=====< 0x14603c11a      jz    0x14603c192
|   |||||   0x14603c11c      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|   |||||   0x14603c123      cmp   al, 0x06                            ; 6
|  ,======< 0x14603c125      jb    0x14603c192
|  ||||||   0x14603c127      shr   rax, 0x08
|  ||||||   0x14603c12b      cmp   al, 0x05                            ; 5
| ,=======< 0x14603c12d      jb    0x14603c192
| |||||||   0x14603c12f      lea   rax, qword [0x148f730e0]            ; "[DFLog::RbxTransportClientLog] Both channels ready, firing connectionReady"
| |||||||   0x14603c136      mov   qword [var_308h], rax
| |||||||   0x14603c13b      mov   qword [var_300h], 0x4a              ; 'J'
| |||||||                                                              ; [0x4a:8]=-1 ; 74
| |||||||   0x14603c144      mov   qword [var_328h], r12
| |||||||   0x14603c149      lea   rax, qword [var_f8h]
| |||||||   0x14603c150      mov   qword [var_320h], rax
| |||||||   0x14603c155      movaps xmm0, xmmword [var_328h]
| |||||||   0x14603c15a      movdqa xmmword [var_328h], xmm0
| |||||||   0x14603c160      movaps xmm1, xmmword [var_308h]
| |||||||   0x14603c165      movdqa xmmword [var_308h], xmm1
| |||||||   0x14603c16b      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| |||||||   0x14603c172      movaps xmmword [var_318h], xmm0
| |||||||   0x14603c177      mov   byte [var_348h], 0x01
| |||||||   0x14603c17c      lea   r9, qword [var_328h]
| |||||||   0x14603c181      lea   r8, qword [var_308h]
| |||||||   0x14603c186      mov   dl, 0x05
| |||||||   0x14603c188      lea   rcx, qword [var_318h]
| |||||||   0x14603c18d      call  0x1438602b0
| ```-----> 0x14603c192      lea   rcx, qword [rsi+0x18]
|    ||||   0x14603c196      mov   byte [var_338h], 0x01
|    ||||   0x14603c19b      lea   rdx, qword [var_338h]
|    ||||   0x14603c1a0      call  0x14097c980
|    |||`-> 0x14603c1a5      mov   bl, 0x01
|    |||    ; CODE XREF from fcn.14603bb70 @ 0x14603bfc6
|    `----> 0x14603c1a7      lea   rcx, qword [var_60h]
|     ||    0x14603c1ae      call  0x14073a8e0
|     ||    0x14603c1b3      movzx eax, bl
|     ||    ; CODE XREFS from fcn.14603bb70 @ 0x14603bc2d, 0x14603bd4c
|     ``--> 0x14603c1b6      mov   rcx, qword [var_38h]
|           0x14603c1bd      xor   rcx, rsp
|           0x14603c1c0      call  0x14730fca0
|           0x14603c1c5      lea   r11, qword [var_28h]
|           0x14603c1cd      mov   rbx, qword [r11+0x38]
|           0x14603c1d1      mov   rsi, qword [r11+0x40]
|           0x14603c1d5      mov   rsp, r11
|           0x14603c1d8      pop   r15
|           0x14603c1da      pop   r14
|           0x14603c1dc      pop   r12
|           0x14603c1de      pop   rdi
|           0x14603c1df      pop   rbp
\           0x14603c1e0      ret
