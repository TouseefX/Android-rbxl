/ fcn.14603e910(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_78h @ stack - 0x78
|           ; var int64_t var_70h @ stack - 0x70
|           ; var int64_t var_68h @ stack - 0x68
|           ; var int64_t var_60h @ stack - 0x60
|           ; var int64_t var_58h @ stack - 0x58
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_30h @ stack - 0x30
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_18h @ stack + 0x18
|           ; var int64_t var_20h @ stack + 0x20
|           0x14603e910      mov   qword [var_18h], rbx
|           0x14603e915      mov   qword [var_20h], rbp
|           0x14603e91a      push  rsi
|           0x14603e91b      push  rdi
|           0x14603e91c      push  r14
|           0x14603e91e      sub   rsp, 0xa0
|           0x14603e925      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14603e92c      xor   rax, rsp
|           0x14603e92f      mov   qword [var_28h], rax
|           0x14603e937      mov   r14, rdx                            ; arg2
|           0x14603e93a      mov   rsi, rcx                            ; arg1
|           0x14603e93d      mov   eax, 0x01
|           0x14603e942      xchg  byte [rcx+0x80], al                 ; arg1
|           0x14603e948      xor   ebp, ebp
|           0x14603e94a      mov   eax, ebp
|           0x14603e94c      xchg  byte [rcx+0x09], al                 ; arg1
|           0x14603e94f      add   rcx, 0x78                           ; 120 ; arg1
|           0x14603e953      call  0x1427c8110
|           0x14603e958      test  al, al
|       ,=< 0x14603e95a      jz    0x14603e965
|       |   0x14603e95c      lea   rcx, qword [rsi+0x78]
|       |   0x14603e960      call  0x1427c80f0
|       `-> 0x14603e965      lea   rbx, qword [rsi+0x50]
|           0x14603e969      mov   rcx, rbx
|           0x14603e96c      call  fcn.1427c8210
|           0x14603e971      mov   word [rsi+0x81], bp
|           0x14603e978      mov   rcx, rbx
|           0x14603e97b      call  fcn.1427c8240
|           0x14603e980      nop
|           0x14603e981      lea   rcx, qword [rsi+0x18]
|           0x14603e985      mov   byte [var_88h], 0x00
|           0x14603e98a      lea   rdx, qword [var_88h]
|           0x14603e98f      call  0x14097c980
|           0x14603e994      mov   qword [var_48h], rbx
|           0x14603e999      mov   rcx, rbx
|           0x14603e99c      call  fcn.1427c8210
|           0x14603e9a1      nop
|           0x14603e9a2      mov   rcx, qword [rsi+0x58]
|           0x14603e9a6      test  rcx, rcx
|       ,=< 0x14603e9a9      jz    0x14603e9b9
|       |   0x14603e9ab      mov   rax, qword [rcx]
|       |   0x14603e9ae      mov   r8, r14
|       |   0x14603e9b1      mov   edx, 0x01
|       |   0x14603e9b6      call  qword [rax+0x78]                    ; 120
|       `-> 0x14603e9b9      lea   r8, qword [rsi+0x88]
|           0x14603e9c0      mov   rdx, qword [rsi+0x90]
|           0x14603e9c7      mov   rcx, qword [rsi+0x88]
|           0x14603e9ce      call  0x140734550
|           0x14603e9d3      mov   rax, qword [rsi+0x88]
|           0x14603e9da      mov   qword [rsi+0x90], rax
|           0x14603e9e1      mov   rcx, rbx
|           0x14603e9e4      call  fcn.1427c8240
|           0x14603e9e9      nop
|           0x14603e9ea      movzx eax, byte [rsi+0x20]
|           0x14603e9ee      nop
|           0x14603e9ef      test  al, al
|       ,=< 0x14603e9f1      jz    0x14603eac0
|       |   0x14603e9f7      xchg  byte [rsi+0x20], bpl
|       |   0x14603e9fb      mov   eax, dword [r14]
|       |   0x14603e9fe      mov   dword [rsi+0x28], eax
|       |   0x14603ea01      mov   eax, dword [r14+0x04]
|       |   0x14603ea05      mov   dword [rsi+0x2c], eax
|       |   0x14603ea08      lea   rdx, qword [r14+0x08]
|       |   0x14603ea0c      lea   rcx, qword [rsi+0x30]
|       |   0x14603ea10      cmp   rcx, rdx
|      ,==< 0x14603ea13      jz    0x14603ea28
|      ||   0x14603ea15      mov   r8, qword [rdx+0x10]
|      ||   0x14603ea19      cmp   qword [rdx+0x18], 0x10
|     ,===< 0x14603ea1e      jb    0x14603ea23
|     |||   0x14603ea20      mov   rdx, qword [rdx]
|     `---> 0x14603ea23      call  0x1407425a0
|      `--> 0x14603ea28      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|       |   0x14603ea2f      cmp   al, 0x06                            ; 6
|      ,==< 0x14603ea31      jb    0x14603eb40
|      ||   0x14603ea37      shr   rax, 0x08
|      ||   0x14603ea3b      cmp   al, 0x04                            ; 4
|     ,===< 0x14603ea3d      jb    0x14603eb40
|     |||   0x14603ea43      lea   rax, qword [0x148f73130]            ; "[DFLog::RbxTransportClientLog] RbxTransportClient disconnected from server for reason: {}"
|     |||   0x14603ea4a      mov   qword [var_68h], rax
|     |||   0x14603ea4f      mov   qword [var_60h], 0x59               ; 'Y'
|     |||                                                              ; [0x59:8]=-1 ; 89
|     |||   0x14603ea58      mov   qword [var_38h], r14
|     |||   0x14603ea60      lea   rax, qword [0x1435e0c40]
|     |||   0x14603ea67      mov   qword [var_30h], rax
|     |||   0x14603ea6f      movaps xmm0, xmmword [var_38h]
|     |||   0x14603ea77      movdqa xmmword [var_38h], xmm0
|     |||   0x14603ea80      mov   qword [var_78h], 0x0f               ; [0xf:8]=-1 ; 15
|     |||   0x14603ea89      lea   rax, qword [var_38h]
|     |||   0x14603ea91      mov   qword [var_70h], rax
|     |||   0x14603ea96      movaps xmm0, xmmword [var_78h]
|     |||   0x14603ea9b      movdqa xmmword [var_78h], xmm0
|     |||   0x14603eaa1      movaps xmm1, xmmword [var_68h]
|     |||   0x14603eaa6      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|     |||   0x14603eaad      movaps xmmword [var_58h], xmm0
|     |||   0x14603eab2      lea   r9, qword [var_78h]
|     |||   0x14603eab7      mov   dl, 0x04
|     |||   0x14603eab9      lea   rcx, qword [var_58h]
|    ,====< 0x14603eabe      jmp   0x14603eb2b
|    |||`-> 0x14603eac0      cmp   byte [0x14d920570], 0x00            ; [0x14d920570:1]=0
|    |||,=< 0x14603eac7      jz    0x14603eb40
|    ||||   0x14603eac9      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|    ||||   0x14603ead0      cmp   al, 0x06                            ; 6
|   ,=====< 0x14603ead2      jb    0x14603eb40
|   |||||   0x14603ead4      shr   rax, 0x08
|   |||||   0x14603ead8      cmp   al, 0x05                            ; 5
|  ,======< 0x14603eada      jb    0x14603eb40
|  ||||||   0x14603eadc      lea   rax, qword [0x148f73190]            ; "[DFLog::RbxTransportClientLog] Disconnect called but was not connected"
|  ||||||   0x14603eae3      mov   qword [var_78h], rax
|  ||||||   0x14603eae8      mov   qword [var_70h], 0x46               ; 'F'
|  ||||||                                                              ; [0x46:8]=-1 ; 70
|  ||||||   0x14603eaf1      mov   qword [var_68h], rbp
|  ||||||   0x14603eaf6      lea   rax, qword [var_38h]
|  ||||||   0x14603eafe      mov   qword [var_60h], rax
|  ||||||   0x14603eb03      movaps xmm0, xmmword [var_68h]
|  ||||||   0x14603eb08      movdqa xmmword [var_58h], xmm0
|  ||||||   0x14603eb0e      movaps xmm1, xmmword [var_78h]
|  ||||||   0x14603eb13      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|  ||||||   0x14603eb1a      movaps xmmword [var_78h], xmm0
|  ||||||   0x14603eb1f      lea   r9, qword [var_58h]
|  ||||||   0x14603eb24      mov   dl, 0x05
|  ||||||   0x14603eb26      lea   rcx, qword [var_78h]
|  ||||||   ; CODE XREF from fcn.14603e910 @ 0x14603eabe
|  ||`----> 0x14603eb2b      mov   byte [var_98h], 0x01
|  || |||   0x14603eb30      movdqa xmmword [var_68h], xmm1
|  || |||   0x14603eb36      lea   r8, qword [var_68h]
|  || |||   0x14603eb3b      call  0x1438602b0
|  ``-```-> 0x14603eb40      mov   rcx, qword [var_28h]
|           0x14603eb48      xor   rcx, rsp
|           0x14603eb4b      call  0x14730fca0
|           0x14603eb50      lea   r11, qword [var_28h + 0x10]
|           0x14603eb58      mov   rbx, qword [r11+0x30]
|           0x14603eb5c      mov   rbp, qword [r11+0x38]
|           0x14603eb60      mov   rsp, r11
|           0x14603eb63      pop   r14
|           0x14603eb65      pop   rdi
|           0x14603eb66      pop   rsi
\           0x14603eb67      ret
