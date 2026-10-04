/ fcn.14603eb70(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_3a8h @ stack - 0x3a8
|           ; var int64_t var_398h @ stack - 0x398
|           ; var int64_t var_390h @ stack - 0x390
|           ; var int64_t var_388h @ stack - 0x388
|           ; var int64_t var_380h @ stack - 0x380
|           ; var int64_t var_378h @ stack - 0x378
|           ; var int64_t var_370h @ stack - 0x370
|           ; var int64_t var_368h @ stack - 0x368
|           ; var int64_t var_358h @ stack - 0x358
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
|           ; var int64_t var_288h @ stack - 0x288
|           ; var int64_t var_280h @ stack - 0x280
|           ; var int64_t var_278h @ stack - 0x278
|           ; var int64_t var_260h @ stack - 0x260
|           ; var int64_t var_25ch @ stack - 0x25c
|           ; var int64_t var_258h @ stack - 0x258
|           ; var int64_t var_238h @ stack - 0x238
|           ; var int64_t var_1e0h @ stack - 0x1e0
|           ; var int64_t var_1d8h @ stack - 0x1d8
|           ; var int64_t var_1d0h @ stack - 0x1d0
|           ; var int64_t var_1c8h @ stack - 0x1c8
|           ; var int64_t var_1c0h @ stack - 0x1c0
|           ; var int64_t var_1bch @ stack - 0x1bc
|           ; var int64_t var_1b8h @ stack - 0x1b8
|           ; var int64_t var_1a8h @ stack - 0x1a8
|           ; var int64_t var_1a0h @ stack - 0x1a0
|           ; var int64_t var_198h @ stack - 0x198
|           ; var int64_t var_190h @ stack - 0x190
|           ; var int64_t var_18ch @ stack - 0x18c
|           ; var int64_t var_188h @ stack - 0x188
|           ; var int64_t var_178h @ stack - 0x178
|           ; var int64_t var_170h @ stack - 0x170
|           ; var int64_t var_168h @ stack - 0x168
|           ; var int64_t var_158h @ stack - 0x158
|           ; var int64_t var_150h @ stack - 0x150
|           ; var int64_t var_138h @ stack - 0x138
|           ; var int64_t var_130h @ stack - 0x130
|           ; var int64_t var_120h @ stack - 0x120
|           ; var int64_t var_110h @ stack - 0x110
|           ; var int64_t var_f8h @ stack - 0xf8
|           ; var int64_t var_e0h @ stack - 0xe0
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_60h @ stack - 0x60
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_18h @ stack + 0x18
|           0x14603eb70      mov   qword [var_18h], rbx
|           0x14603eb75      push  rbp
|           0x14603eb76      push  rsi
|           0x14603eb77      push  rdi
|           0x14603eb78      push  r12
|           0x14603eb7a      push  r13
|           0x14603eb7c      push  r14
|           0x14603eb7e      push  r15
|           0x14603eb80      lea   rbp, qword [var_2c8h]
|           0x14603eb88      sub   rsp, 0x390
|           0x14603eb8f      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14603eb96      xor   rax, rsp
|           0x14603eb99      mov   qword [var_48h], rax
|           0x14603eba0      mov   r12, rdx                            ; arg2
|           0x14603eba3      mov   r13, rcx                            ; arg1
|           0x14603eba6      mov   qword [var_370h], rdx               ; arg2
|           0x14603ebab      xor   edi, edi
|           0x14603ebad      mov   r14d, edi
|           0x14603ebb0      mov   dword [var_338h], edi
|           0x14603ebb3      mov   qword [var_298h], rdi
|           0x14603ebb7      mov   qword [var_280h], 0x0f              ; 0xf ; 15
|           0x14603ebbf      mov   qword [var_288h], rdi
|           0x14603ebc3      lea   r9, qword [var_298h]
|           0x14603ebc7      xor   r8d, r8d
|           0x14603ebca      xor   edx, edx
|           0x14603ebcc      lea   rcx, qword [var_158h]
|           0x14603ebd3      call  fcn.1427c28c0
|           0x14603ebd8      mov   rcx, rax
|           0x14603ebdb      mov   byte [var_1c8h], dil
|           0x14603ebe2      mov   eax, dword [rax]
|           0x14603ebe4      mov   dword [var_1c0h], eax
|           0x14603ebea      mov   eax, dword [rcx+0x04]
|           0x14603ebed      mov   dword [var_1bch], eax
|           0x14603ebf3      movups xmm0, xmmword [rcx+0x08]
|           0x14603ebf7      movups xmmword [var_1b8h], xmm0
|           0x14603ebfe      movups xmm1, xmmword [rcx+0x18]
|           0x14603ec02      movups xmmword [var_1a8h], xmm1
|           0x14603ec09      mov   qword [rcx+0x18], rdi
|           0x14603ec0d      mov   qword [rcx+0x20], 0x0f              ; [0xf:8]=-1 ; 15
|           0x14603ec15      mov   byte [rcx+0x08], dil
|           0x14603ec19      mov   rdx, qword [var_138h]
|           0x14603ec20      cmp   rdx, 0x10                           ; 16
|       ,=< 0x14603ec24      jb    0x14603ec5d
|       |   0x14603ec26      inc   rdx
|       |   0x14603ec29      mov   rcx, qword [var_150h]
|       |   0x14603ec30      mov   rax, rcx
|       |   0x14603ec33      cmp   rdx, 0x1000
|      ,==< 0x14603ec3a      jb    0x14603ec58
|      ||   0x14603ec3c      add   rdx, 0x27                           ; 39
|      ||   0x14603ec40      mov   rcx, qword [rcx-0x08]
|      ||   0x14603ec44      sub   rax, rcx
|      ||   0x14603ec47      add   rax, 0xfffffffffffffff8
|      ||   0x14603ec4b      cmp   rax, 0x1f                           ; 31
|     ,===< 0x14603ec4f      jbe   0x14603ec58
|     |||   0x14603ec51      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|     |||   0x14603ec57      int3
|     ``--> 0x14603ec58      call  fcn.14385bd60
|       `-> 0x14603ec5d      movdqa xmm0, xmmword [0x1484ae800]
|           0x14603ec65      movdqu xmmword [var_150h + 0x10], xmm0
|           0x14603ec6d      mov   byte [var_150h], 0x00
|           0x14603ec74      mov   dword [var_370h], edi
|           0x14603ec78      cmp   dword [0x14c421550], 0x00           ; [0x14c421550:4]=0x7d0
|       ,=< 0x14603ec7f      jle   0x14603f350
|      .--> 0x14603ec85      movzx eax, byte [r13+0x80]
|      :|   0x14603ec8d      nop
|      :|   0x14603ec8e      test  al, al
|     ,===< 0x14603ec90      jnz   0x14603f350
|     |:|   0x14603ec96      xor   r15b, r15b
|     |:|   0x14603ec99      xorps xmm0, xmm0
|     |:|   0x14603ec9c      movdqu xmmword [var_398h], xmm0
|     |:|   0x14603eca2      mov   qword [var_388h], rdi
|     |:|   0x14603eca7      mov   qword [var_380h], rdi
|     |:|   0x14603ecac      mov   qword [var_378h], rdi
|     |:|   0x14603ecb1      mov   ecx, 0x10                           ; 16
|     |:|   0x14603ecb6      call  fcn.14385bce0
|     |:|   0x14603ecbb      mov   qword [rax+0x08], rdi
|     |:|   0x14603ecbf      mov   qword [var_398h], rax
|     |:|   0x14603ecc4      lea   rcx, qword [var_398h]
|     |:|   0x14603ecc9      mov   qword [rax], rcx
|     |:|   0x14603eccc      lea   rbx, qword [r13+0x50]
|     |:|   0x14603ecd0      mov   qword [var_358h], rbx
|     |:|   0x14603ecd5      mov   rcx, rbx
|     |:|   0x14603ecd8      call  fcn.1427c8210
|     |:|   0x14603ecdd      nop
|     |:|   0x14603ecde      mov   rcx, qword [r13+0x58]
|     |:|   0x14603ece2      mov   rax, qword [rcx]
|     |:|   0x14603ece5      lea   rdx, qword [var_198h]
|     |:|   0x14603ecec      call  qword [rax+0x10]                    ; 16
|     |:|   0x14603ecef      mov   rsi, rax
|     |:|   0x14603ecf2      lea   rax, qword [var_398h]
|     |:|   0x14603ecf7      cmp   rax, rsi
|    ,====< 0x14603ecfa      jz    0x14603ee4c
|    ||:|   0x14603ed00      mov   rcx, qword [var_378h]
|    ||:|   0x14603ed05      test  rcx, rcx
|   ,=====< 0x14603ed08      jz    0x14603ed68
|   |||:|   0x14603ed0a      nop   word [rax+rax*1], ax
|  .------> 0x14603ed10      mov   rax, qword [var_380h]
|  :|||:|   0x14603ed15      dec   rax
|  :|||:|   0x14603ed18      add   rcx, rax
|  :|||:|   0x14603ed1b      mov   rdx, qword [var_388h]
|  :|||:|   0x14603ed20      dec   rdx
|  :|||:|   0x14603ed23      and   rdx, rcx
|  :|||:|   0x14603ed26      mov   rax, qword [var_390h]
|  :|||:|   0x14603ed2b      mov   rdi, qword [rax+rdx*8]
|  :|||:|   0x14603ed2f      lea   rcx, qword [rdi+0x40]
|  :|||:|   0x14603ed33      cmp   byte [rcx+0x58], 0x00
| ,=======< 0x14603ed37      jz    0x14603ed3e
| |:|||:|   0x14603ed39      call  fcn.1435d0810
| `-------> 0x14603ed3e      lea   rcx, qword [rdi+0x18]
|  :|||:|   0x14603ed42      call  fcn.14073a8e0
|  :|||:|   0x14603ed47      mov   rcx, qword [rdi]
|  :|||:|   0x14603ed4a      call  fcn.14385bd60
|  :|||:|   0x14603ed4f      mov   rcx, qword [var_378h]
|  :|||:|   0x14603ed54      sub   rcx, 0x01
|  :|||:|   0x14603ed58      mov   qword [var_378h], rcx
|  `======< 0x14603ed5d      jnz   0x14603ed10
|   |||:|   0x14603ed5f      mov   qword [var_380h], 0x00
|   `-----> 0x14603ed68      mov   rdi, qword [var_388h]
|    ||:|   0x14603ed6d      mov   rcx, qword [var_390h]
|    ||:|   0x14603ed72      test  rdi, rdi
|   ,=====< 0x14603ed75      jz    0x14603eda8
|   |||:|   0x14603ed77      nop   word [rax+rax*1], ax
|  .------> 0x14603ed80      dec   rdi
|  :|||:|   0x14603ed83      mov   rax, qword [rcx+rdi*8]
|  :|||:|   0x14603ed87      test  rax, rax
| ,=======< 0x14603ed8a      jz    0x14603ed9e
| |:|||:|   0x14603ed8c      mov   edx, 0xa8                           ; 168
| |:|||:|   0x14603ed91      mov   rcx, rax
| |:|||:|   0x14603ed94      call  fcn.14385bd60
| |:|||:|   0x14603ed99      mov   rcx, qword [var_390h]
| `-------> 0x14603ed9e      test  rdi, rdi
|  `======< 0x14603eda1      jnz   0x14603ed80
|   |||:|   0x14603eda3      mov   rdi, qword [var_388h]
|   `-----> 0x14603eda8      test  rcx, rcx
|   ,=====< 0x14603edab      jz    0x14603eddf
|   |||:|   0x14603edad      lea   rdx, qword [rdi*8]
|   |||:|   0x14603edb5      mov   rax, rcx
|   |||:|   0x14603edb8      cmp   rdx, 0x1000
|  ,======< 0x14603edbf      jb    0x14603edda
|  ||||:|   0x14603edc1      add   rdx, 0x27                           ; 39
|  ||||:|   0x14603edc5      mov   rcx, qword [rcx-0x08]
|  ||||:|   0x14603edc9      sub   rax, rcx
|  ||||:|   0x14603edcc      add   rax, 0xfffffffffffffff8
|  ||||:|   0x14603edd0      cmp   rax, 0x1f                           ; 31
| ,=======< 0x14603edd4      jnbe  0x14603f44f
| |`------> 0x14603edda      call  fcn.14385bd60
| | `-----> 0x14603eddf      xor   edi, edi
| |  ||:|   0x14603ede1      mov   qword [var_388h], rdi
| |  ||:|   0x14603ede6      mov   qword [var_390h], rdi
| |  ||:|   0x14603edeb      mov   rcx, qword [var_398h]
| |  ||:|   0x14603edf0      mov   rax, qword [rsi]
| |  ||:|   0x14603edf3      mov   qword [var_398h], rax
| |  ||:|   0x14603edf8      mov   qword [rsi], rcx
| |  ||:|   0x14603edfb      mov   rax, qword [var_398h]
| |  ||:|   0x14603ee00      test  rax, rax
| | ,=====< 0x14603ee03      jz    0x14603ee10
| | |||:|   0x14603ee05      lea   rcx, qword [var_398h]
| | |||:|   0x14603ee0a      mov   qword [rax], rcx
| | |||:|   0x14603ee0d      mov   rcx, qword [rsi]
| | `-----> 0x14603ee10      test  rcx, rcx
| | ,=====< 0x14603ee13      jz    0x14603ee18
| | |||:|   0x14603ee15      mov   qword [rcx], rsi
| | `-----> 0x14603ee18      mov   rax, qword [rsi+0x08]
| |  ||:|   0x14603ee1c      mov   qword [var_390h], rax
| |  ||:|   0x14603ee21      mov   rax, qword [rsi+0x10]
| |  ||:|   0x14603ee25      mov   qword [var_388h], rax
| |  ||:|   0x14603ee2a      mov   rax, qword [rsi+0x18]
| |  ||:|   0x14603ee2e      mov   qword [var_380h], rax
| |  ||:|   0x14603ee33      mov   rax, qword [rsi+0x20]
| |  ||:|   0x14603ee37      mov   qword [var_378h], rax
| |  ||:|   0x14603ee3c      mov   qword [rsi+0x08], rdi
| |  ||:|   0x14603ee40      mov   qword [rsi+0x10], rdi
| |  ||:|   0x14603ee44      mov   qword [rsi+0x18], rdi
| |  ||:|   0x14603ee48      mov   qword [rsi+0x20], rdi
| |  `----> 0x14603ee4c      lea   rcx, qword [var_198h]
| |   |:|   0x14603ee53      call  fcn.1435dc900
| |   |:|   0x14603ee58      mov   rcx, qword [var_198h]
| |   |:|   0x14603ee5f      mov   qword [var_198h], rdi
| |   |:|   0x14603ee66      mov   edx, 0x10                           ; 16
| |   |:|   0x14603ee6b      call  fcn.14385bd60
| |   |:|   0x14603ee70      nop
| |   |:|   0x14603ee71      mov   rcx, rbx
| |   |:|   0x14603ee74      call  fcn.1427c8240
| |   |:|   0x14603ee79      nop
| |   |:|   0x14603ee7a      cmp   qword [var_378h], 0x00
| |  ,====< 0x14603ee80      jz    0x14603f2ff
| | .-----> 0x14603ee86      mov   rdx, qword [var_388h]
| | :||:|   0x14603ee8b      dec   rdx
| | :||:|   0x14603ee8e      and   rdx, qword [var_380h]
| | :||:|   0x14603ee93      mov   rax, qword [var_390h]
| | :||:|   0x14603ee98      mov   rdx, qword [rax+rdx*8]
| | :||:|   0x14603ee9c      lea   rcx, qword [var_f8h]
| | :||:|   0x14603eea3      call  fcn.1435e2050
| | :||:|   0x14603eea8      nop
| | :||:|   0x14603eea9      mov   rcx, qword [var_388h]
| | :||:|   0x14603eeae      dec   rcx
| | :||:|   0x14603eeb1      and   rcx, qword [var_380h]
| | :||:|   0x14603eeb6      mov   rax, qword [var_390h]
| | :||:|   0x14603eebb      mov   rdi, qword [rax+rcx*8]
| | :||:|   0x14603eebf      lea   rcx, qword [rdi+0x40]
| | :||:|   0x14603eec3      cmp   byte [rcx+0x58], 0x00
| |,======< 0x14603eec7      jz    0x14603eece
| ||:||:|   0x14603eec9      call  fcn.1435d0810
| |`------> 0x14603eece      lea   rcx, qword [rdi+0x18]
| | :||:|   0x14603eed2      call  fcn.14073a8e0
| | :||:|   0x14603eed7      mov   rcx, qword [rdi]
| | :||:|   0x14603eeda      call  fcn.14385bd60
| | :||:|   0x14603eedf      sub   qword [var_378h], 0x01
| |,======< 0x14603eee5      jnz   0x14603eef0
| ||:||:|   0x14603eee7      xor   edi, edi
| ||:||:|   0x14603eee9      mov   qword [var_380h], rdi
| ========< 0x14603eeee      jmp   0x14603eef7
| |`------> 0x14603eef0      inc   qword [var_380h]
| | :||:|   0x14603eef5      xor   edi, edi
| | :||:|   ; CODE XREF from fcn.14603eb70 @ 0x14603eeee
| --------> 0x14603eef7      lea   rdx, qword [var_f8h]
| | :||:|   0x14603eefe      lea   rcx, qword [var_278h]
| | :||:|   0x14603ef02      call  fcn.1435e2050
| | :||:|   0x14603ef07      mov   byte [var_1d0h], 0x01
| | :||:|   0x14603ef0e      cmp   byte [var_60h], 0x00
| |,======< 0x14603ef15      jz    0x14603ef23
| ||:||:|   0x14603ef17      lea   rcx, qword [var_b8h]
| ||:||:|   0x14603ef1e      call  fcn.1435d0810
| |`------> 0x14603ef23      lea   rcx, qword [var_e0h]
| | :||:|   0x14603ef2a      call  fcn.14073a8e0
| | :||:|   0x14603ef2f      mov   rcx, qword [var_f8h]
| | :||:|   0x14603ef36      call  fcn.14385bd60
| | :||:|   0x14603ef3b      nop
| | :||:|   0x14603ef3c      or    r14d, 0x02
| | :||:|   0x14603ef40      cmp   byte [var_1d0h], 0x00
| |,======< 0x14603ef47      jnz   0x14603ef4e
| ========< 0x14603ef49      jmp   0x14603f2ea
| |`------> 0x14603ef4e      movzx eax, byte [var_1d8h]
| | :||:|   0x14603ef55      test  al, al
| |,======< 0x14603ef57      jnz   0x14603f0ab
| ||:||:|   0x14603ef5d      mov   r15b, 0x01
| ||:||:|   0x14603ef60      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
| ||:||:|   0x14603ef67      cmp   al, 0x06                            ; 6
| ========< 0x14603ef69      jb    0x14603efd4
| ||:||:|   0x14603ef6b      shr   rax, 0x08
| ||:||:|   0x14603ef6f      cmp   al, 0x04                            ; 4
| ========< 0x14603ef71      jb    0x14603efd4
| ||:||:|   0x14603ef73      lea   rax, qword [0x148f72d70]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient connection opened."
| ||:||:|   0x14603ef7a      mov   qword [var_318h], rax
| ||:||:|   0x14603ef7e      mov   qword [var_310h], 0x44              ; 'D' ; 68
| ||:||:|   0x14603ef86      mov   qword [var_328h], rdi
| ||:||:|   0x14603ef8a      lea   rax, qword [var_130h]
| ||:||:|   0x14603ef91      mov   qword [var_320h], rax
| ||:||:|   0x14603ef95      movaps xmm0, xmmword [var_328h]
| ||:||:|   0x14603ef99      movdqa xmmword [var_358h], xmm0
| ||:||:|   0x14603ef9f      movaps xmm1, xmmword [var_318h]
| ||:||:|   0x14603efa3      movdqa xmmword [var_368h], xmm1
| ||:||:|   0x14603efa9      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| ||:||:|   0x14603efb0      movaps xmmword [var_168h], xmm0
| ||:||:|   0x14603efb7      mov   byte [var_3a8h], r15b
| ||:||:|   0x14603efbc      lea   r9, qword [var_358h]
| ||:||:|   0x14603efc1      lea   r8, qword [var_368h]
| ||:||:|   0x14603efc6      mov   dl, 0x04
| ||:||:|   0x14603efc8      lea   rcx, qword [var_168h]
| ||:||:|   0x14603efcf      call  fcn.1438602b0
| --------> 0x14603efd4      mov   byte [var_198h], r15b
| ||:||:|   0x14603efdb      mov   eax, dword [var_260h]
| ||:||:|   0x14603efde      mov   dword [var_190h], eax
| ||:||:|   0x14603efe4      mov   eax, dword [var_25ch]
| ||:||:|   0x14603efe7      mov   dword [var_18ch], eax
| ||:||:|   0x14603efed      lea   rdx, qword [var_258h]
| ||:||:|   0x14603eff1      lea   rcx, qword [var_188h]
| ||:||:|   0x14603eff8      call  fcn.140739200
| ||:||:|   0x14603effd      movzx eax, byte [var_198h]
| ||:||:|   0x14603f004      mov   byte [var_1c8h], al
| ||:||:|   0x14603f00a      mov   eax, dword [var_190h]
| ||:||:|   0x14603f010      mov   dword [var_1c0h], eax
| ||:||:|   0x14603f016      mov   eax, dword [var_18ch]
| ||:||:|   0x14603f01c      mov   dword [var_1bch], eax
| ||:||:|   0x14603f022      mov   rdx, qword [var_1a0h]
| ||:||:|   0x14603f029      cmp   rdx, 0x10                           ; 16
| ========< 0x14603f02d      jb    0x14603f063
| ||:||:|   0x14603f02f      inc   rdx
| ||:||:|   0x14603f032      mov   rcx, qword [var_1b8h]
| ||:||:|   0x14603f039      mov   rax, rcx
| ||:||:|   0x14603f03c      cmp   rdx, 0x1000
| ========< 0x14603f043      jb    0x14603f05e
| ||:||:|   0x14603f045      add   rdx, 0x27                           ; 39
| ||:||:|   0x14603f049      mov   rcx, qword [rcx-0x08]
| ||:||:|   0x14603f04d      sub   rax, rcx
| ||:||:|   0x14603f050      add   rax, 0xfffffffffffffff8
| ||:||:|   0x14603f054      cmp   rax, 0x1f                           ; 31
| ========< 0x14603f058      jnbe  0x14603f456
| --------> 0x14603f05e      call  fcn.14385bd60
| --------> 0x14603f063      movups xmm0, xmmword [var_188h]
| ||:||:|   0x14603f06a      movups xmmword [var_1b8h], xmm0
| ||:||:|   0x14603f071      movups xmm1, xmmword [var_178h]
| ||:||:|   0x14603f078      movups xmmword [var_1a8h], xmm1
| ||:||:|   0x14603f07f      mov   qword [var_178h], rdi
| ||:||:|   0x14603f086      mov   qword [var_170h], 0x0f              ; 0xf ; 15
| ||:||:|   0x14603f091      mov   byte [var_188h], 0x00
| ||:||:|   0x14603f098      lea   rcx, qword [var_190h]
| ||:||:|   0x14603f09f      call  fcn.14073a8e0
| ||:||:|   0x14603f0a4      movzx eax, byte [var_1d8h]
| |`------> 0x14603f0ab      cmp   al, 0x02                            ; 2
| |,======< 0x14603f0ad      jnz   0x14603f16c
| ||:||:|   0x14603f0b3      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
| ||:||:|   0x14603f0ba      cmp   al, 0x06                            ; 6
| ========< 0x14603f0bc      jb    0x14603f14c
| ||:||:|   0x14603f0c2      shr   rax, 0x08
| ||:||:|   0x14603f0c6      cmp   al, 0x04                            ; 4
| ========< 0x14603f0c8      jb    0x14603f14c
| ||:||:|   0x14603f0ce      lea   rax, qword [0x148f72dc0]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient ACK ReceiveChannelOpened IN WaitForConnection {}"
| ||:||:|   0x14603f0d5      mov   qword [var_2e8h], rax
| ||:||:|   0x14603f0d9      mov   qword [var_2e0h], 0x62              ; 'b' ; 98
| ||:||:|   0x14603f0e1      lea   rax, qword [var_260h]
| ||:||:|   0x14603f0e5      mov   qword [var_308h], rax
| ||:||:|   0x14603f0e9      lea   rax, qword [0x1435e0c40]
| ||:||:|   0x14603f0f0      mov   qword [var_300h], rax
| ||:||:|   0x14603f0f4      movaps xmm0, xmmword [var_308h]
| ||:||:|   0x14603f0f8      movdqa xmmword [var_168h], xmm0
| ||:||:|   0x14603f100      mov   qword [var_2f8h], 0x0f              ; 0xf ; 15
| ||:||:|   0x14603f108      lea   rax, qword [var_168h]
| ||:||:|   0x14603f10f      mov   qword [var_2f0h], rax
| ||:||:|   0x14603f113      movaps xmm0, xmmword [var_2f8h]
| ||:||:|   0x14603f117      movdqa xmmword [var_368h], xmm0
| ||:||:|   0x14603f11d      movaps xmm1, xmmword [var_2e8h]
| ||:||:|   0x14603f121      movdqa xmmword [var_358h], xmm1
| ||:||:|   0x14603f127      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| ||:||:|   0x14603f12e      movaps xmmword [var_348h], xmm0
| ||:||:|   0x14603f132      mov   byte [var_3a8h], 0x01
| ||:||:|   0x14603f137      lea   r9, qword [var_368h]
| ||:||:|   0x14603f13c      lea   r8, qword [var_358h]
| ||:||:|   0x14603f141      mov   dl, 0x04
| ||:||:|   0x14603f143      lea   rcx, qword [var_348h]
| ||:||:|   0x14603f147      call  fcn.1438602b0
| --------> 0x14603f14c      mov   rcx, rbx
| ||:||:|   0x14603f14f      call  fcn.1427c8210
| ||:||:|   0x14603f154      mov   byte [r13+0x81], 0x01
| ||:||:|   0x14603f15c      mov   rcx, rbx
| ||:||:|   0x14603f15f      call  fcn.1427c8240
| ||:||:|   0x14603f164      nop
| ||:||:|   0x14603f165      movzx eax, byte [var_1d8h]
| |`------> 0x14603f16c      cmp   al, 0x07                            ; 7
| |,======< 0x14603f16e      jnz   0x14603f2ba
| ||:||:|   0x14603f174      mov   r15b, 0x01
| ||:||:|   0x14603f177      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
| ||:||:|   0x14603f17e      cmp   al, 0x06                            ; 6
| ========< 0x14603f180      jb    0x14603f1e9
| ||:||:|   0x14603f182      shr   rax, 0x08
| ||:||:|   0x14603f186      cmp   al, 0x02                            ; 2
| ========< 0x14603f188      jb    0x14603f1e9
| ||:||:|   0x14603f18a      lea   rax, qword [0x148f72e30]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient connection closed."
| ||:||:|   0x14603f191      mov   qword [rbp], rax
| ||:||:|   0x14603f195      mov   qword [var_2c0h], 0x44              ; 'D' ; 68
| ||:||:|   0x14603f19d      mov   qword [var_2d8h], rdi
| ||:||:|   0x14603f1a1      lea   rax, qword [var_120h]
| ||:||:|   0x14603f1a8      mov   qword [var_2d0h], rax
| ||:||:|   0x14603f1ac      movaps xmm0, xmmword [var_2d8h]
| ||:||:|   0x14603f1b0      movdqa xmmword [var_348h], xmm0
| ||:||:|   0x14603f1b5      movaps xmm1, xmmword [rbp]
| ||:||:|   0x14603f1b9      movdqa xmmword [var_168h], xmm1
| ||:||:|   0x14603f1c1      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| ||:||:|   0x14603f1c8      movaps xmmword [var_368h], xmm0
| ||:||:|   0x14603f1cd      mov   byte [var_3a8h], r15b
| ||:||:|   0x14603f1d2      lea   r9, qword [var_348h]
| ||:||:|   0x14603f1d6      lea   r8, qword [var_168h]
| ||:||:|   0x14603f1dd      mov   dl, 0x02
| ||:||:|   0x14603f1df      lea   rcx, qword [var_368h]
| ||:||:|   0x14603f1e4      call  fcn.1438602b0
| --------> 0x14603f1e9      mov   byte [var_198h], 0x00
| ||:||:|   0x14603f1f0      mov   eax, dword [var_260h]
| ||:||:|   0x14603f1f3      mov   dword [var_190h], eax
| ||:||:|   0x14603f1f9      mov   eax, dword [var_25ch]
| ||:||:|   0x14603f1fc      mov   dword [var_18ch], eax
| ||:||:|   0x14603f202      lea   rdx, qword [var_258h]
| ||:||:|   0x14603f206      lea   rcx, qword [var_188h]
| ||:||:|   0x14603f20d      call  fcn.140739200
| ||:||:|   0x14603f212      movzx eax, byte [var_198h]
| ||:||:|   0x14603f219      mov   byte [var_1c8h], al
| ||:||:|   0x14603f21f      mov   eax, dword [var_190h]
| ||:||:|   0x14603f225      mov   dword [var_1c0h], eax
| ||:||:|   0x14603f22b      mov   eax, dword [var_18ch]
| ||:||:|   0x14603f231      mov   dword [var_1bch], eax
| ||:||:|   0x14603f237      mov   rdx, qword [var_1a0h]
| ||:||:|   0x14603f23e      cmp   rdx, 0x10                           ; 16
| ========< 0x14603f242      jb    0x14603f278
| ||:||:|   0x14603f244      inc   rdx
| ||:||:|   0x14603f247      mov   rcx, qword [var_1b8h]
| ||:||:|   0x14603f24e      mov   rax, rcx
| ||:||:|   0x14603f251      cmp   rdx, 0x1000
| ========< 0x14603f258      jb    0x14603f273
| ||:||:|   0x14603f25a      add   rdx, 0x27                           ; 39
| ||:||:|   0x14603f25e      mov   rcx, qword [rcx-0x08]
| ||:||:|   0x14603f262      sub   rax, rcx
| ||:||:|   0x14603f265      add   rax, 0xfffffffffffffff8
| ||:||:|   0x14603f269      cmp   rax, 0x1f                           ; 31
| ========< 0x14603f26d      jnbe  0x14603f456
| --------> 0x14603f273      call  fcn.14385bd60
| --------> 0x14603f278      movups xmm0, xmmword [var_188h]
| ||:||:|   0x14603f27f      movups xmmword [var_1b8h], xmm0
| ||:||:|   0x14603f286      movups xmm1, xmmword [var_178h]
| ||:||:|   0x14603f28d      movups xmmword [var_1a8h], xmm1
| ||:||:|   0x14603f294      mov   qword [var_178h], rdi
| ||:||:|   0x14603f29b      mov   qword [var_170h], 0x0f              ; 0xf ; 15
| ||:||:|   0x14603f2a6      mov   byte [var_188h], 0x00
| ||:||:|   0x14603f2ad      lea   rcx, qword [var_190h]
| ||:||:|   0x14603f2b4      call  fcn.14073a8e0
| ||:||:|   0x14603f2b9      nop
| |`------> 0x14603f2ba      cmp   byte [var_1d0h], 0x00
| |,======< 0x14603f2c1      jz    0x14603f2ea
| ||:||:|   0x14603f2c3      cmp   byte [var_1e0h], 0x00
| ========< 0x14603f2ca      jz    0x14603f2d8
| ||:||:|   0x14603f2cc      lea   rcx, qword [var_238h]
| ||:||:|   0x14603f2d3      call  fcn.1435d0810
| --------> 0x14603f2d8      lea   rcx, qword [var_260h]
| ||:||:|   0x14603f2dc      call  fcn.14073a8e0
| ||:||:|   0x14603f2e1      mov   rcx, qword [var_278h]
| ||:||:|   0x14603f2e5      call  fcn.14385bd60
| ||:||:|   ; CODE XREF from fcn.14603eb70 @ 0x14603ef49
| -`------> 0x14603f2ea      cmp   qword [var_378h], 0x00
| | `=====< 0x14603f2f0      jnz   0x14603ee86
| |  ||:|   0x14603f2f6      test  r15b, r15b
| | ,=====< 0x14603f2f9      jnz   0x14603f45d
| | |`----> 0x14603f2ff      mov   rcx, qword [r13+0x58]
| | | |:|   0x14603f303      mov   rax, qword [rcx]
| | | |:|   0x14603f306      mov   r8, qword [rax+0x18]
| | | |:|   0x14603f30a      movsxd rax, dword [0x14c421568]           ; [0x14c421568:4]=0x2710
| | | |:|   0x14603f311      imul  rdx, rax, 0x3e8
| | | |:|   0x14603f318      call  r8
| | | |:|   0x14603f31b      nop
| | | |:|   0x14603f31c      lea   rcx, qword [var_398h]
| | | |:|   0x14603f321      call  fcn.1435dc900
| | | |:|   0x14603f326      mov   rcx, qword [var_398h]
| | | |:|   0x14603f32b      mov   qword [var_398h], rdi
| | | |:|   0x14603f330      mov   edx, 0x10                           ; 16
| | | |:|   0x14603f335      call  fcn.14385bd60
| | | |:|   0x14603f33a      mov   ecx, dword [var_370h]
| | | |:|   0x14603f33e      inc   ecx
| | | |:|   0x14603f340      mov   dword [var_370h], ecx
| | | |:|   0x14603f344      cmp   ecx, dword [0x14c421550]            ; [0x14c421550:4]=0x7d0
| | | |`==< 0x14603f34a      jl    0x14603ec85
| | | `-`-> 0x14603f350      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
| | |       0x14603f357      cmp   al, 0x06                            ; 6
| | |   ,=< 0x14603f359      jb    0x14603f3c2
| | |   |   0x14603f35b      shr   rax, 0x08
| | |   |   0x14603f35f      cmp   al, 0x03                            ; 3
| | |  ,==< 0x14603f361      jb    0x14603f3c2
| | |  ||   0x14603f363      lea   rax, qword [0x148f72e78]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient timed out."
| | |  ||   0x14603f36a      mov   qword [var_2a8h], rax
| | |  ||   0x14603f36e      mov   qword [var_2a0h], 0x3c              ; '<' ; 60
| | |  ||   0x14603f376      mov   qword [var_2b8h], rdi
| | |  ||   0x14603f37a      lea   rax, qword [var_110h]
| | |  ||   0x14603f381      mov   qword [var_2b0h], rax
| | |  ||   0x14603f385      movaps xmm0, xmmword [var_2b8h]
| | |  ||   0x14603f389      movdqa xmmword [var_348h], xmm0
| | |  ||   0x14603f38e      movaps xmm1, xmmword [var_2a8h]
| | |  ||   0x14603f392      movdqa xmmword [var_168h], xmm1
| | |  ||   0x14603f39a      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
| | |  ||   0x14603f3a1      movaps xmmword [var_368h], xmm0
| | |  ||   0x14603f3a6      mov   byte [var_3a8h], 0x01
| | |  ||   0x14603f3ab      lea   r9, qword [var_348h]
| | |  ||   0x14603f3af      lea   r8, qword [var_168h]
| | |  ||   0x14603f3b6      mov   dl, 0x03
| | |  ||   0x14603f3b8      lea   rcx, qword [var_368h]
| | |  ||   0x14603f3bd      call  fcn.1438602b0
| | |  ``-> 0x14603f3c2      movzx eax, byte [var_1c8h]
| | |       0x14603f3c9      mov   byte [r12], al
| | |       0x14603f3cd      mov   eax, dword [var_1c0h]
| | |       0x14603f3d3      mov   dword [r12+0x08], eax
| | |       0x14603f3d8      mov   eax, dword [var_1bch]
| | |       0x14603f3de      mov   dword [r12+0x0c], eax
| | |       0x14603f3e3      movups xmm0, xmmword [var_1b8h]
| | |       0x14603f3ea      movups xmmword [r12+0x10], xmm0
| | |       0x14603f3f0      movups xmm1, xmmword [var_1a8h]
| | |       0x14603f3f7      movups xmmword [r12+0x20], xmm1
| | |       0x14603f3fd      mov   qword [var_1a8h], rdi
| | |       0x14603f404      mov   qword [var_1a0h], 0x0f              ; 0xf ; 15
| | |       0x14603f40f      mov   byte [var_1b8h], 0x00
| | |       ; CODE XREF from fcn.14603eb70 @ 0x14603f4cf
| | |   .-> 0x14603f416      lea   rcx, qword [var_1c0h]
| | |   :   0x14603f41d      call  fcn.14073a8e0
| | |   :   0x14603f422      mov   rax, r12
| | |   :   0x14603f425      mov   rcx, qword [var_48h]
| | |   :   0x14603f42c      xor   rcx, rsp
| | |   :   0x14603f42f      call  fcn.14730fca0
| | |   :   0x14603f434      mov   rbx, qword [var_18h]
| | |   :   0x14603f43c      add   rsp, 0x390
| | |   :   0x14603f443      pop   r15
| | |   :   0x14603f445      pop   r14
| | |   :   0x14603f447      pop   r13
| | |   :   0x14603f449      pop   r12
| | |   :   0x14603f44b      pop   rdi
| | |   :   0x14603f44c      pop   rsi
| | |   :   0x14603f44d      pop   rbp
| | |   :   0x14603f44e      ret
| `-------> 0x14603f44f      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|   |   :   0x14603f455      nop
| --------> 0x14603f456      call  qword [sym.imp.api_ms_win_crt_runtime_l1_1_0.dll__invalid_parameter_noinfo_noreturn] ; [0x148442550:8]=0xc35a02e ; ".\xa05\f"
|   |   :   0x14603f45c      nop
|   `-----> 0x14603f45d      movzx eax, byte [var_1c8h]
|       :   0x14603f464      mov   byte [r12], al
|       :   0x14603f468      mov   eax, dword [var_1c0h]
|       :   0x14603f46e      mov   dword [r12+0x08], eax
|       :   0x14603f473      mov   eax, dword [var_1bch]
|       :   0x14603f479      mov   dword [r12+0x0c], eax
|       :   0x14603f47e      movups xmm0, xmmword [var_1b8h]
|       :   0x14603f485      movups xmmword [r12+0x10], xmm0
|       :   0x14603f48b      movups xmm1, xmmword [var_1a8h]
|       :   0x14603f492      movups xmmword [r12+0x20], xmm1
|       :   0x14603f498      mov   qword [var_1a8h], rdi
|       :   0x14603f49f      mov   qword [var_1a0h], 0x0f              ; 0xf ; 15
|       :   0x14603f4aa      mov   byte [var_1b8h], 0x00
|       :   0x14603f4b1      lea   rcx, qword [var_398h]
|       :   0x14603f4b6      call  fcn.1435dc900
|       :   0x14603f4bb      mov   rcx, qword [var_398h]
|       :   0x14603f4c0      mov   qword [var_398h], rdi
|       :   0x14603f4c5      mov   edx, 0x10                           ; 16
|       :   0x14603f4ca      call  fcn.14385bd60
\       `=< 0x14603f4cf      jmp   0x14603f416
