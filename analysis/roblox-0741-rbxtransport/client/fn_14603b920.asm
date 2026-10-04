/ fcn.14603b920(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_138h @ stack - 0x138
|           ; var int64_t var_128h @ stack - 0x128
|           ; var int64_t var_120h @ stack - 0x120
|           ; var int64_t var_118h @ stack - 0x118
|           ; var int64_t var_110h @ stack - 0x110
|           ; var int64_t var_108h @ stack - 0x108
|           ; var int64_t var_f8h @ stack - 0xf8
|           ; var int64_t var_f0h @ stack - 0xf0
|           ; var int64_t var_e8h @ stack - 0xe8
|           ; var int64_t var_e0h @ stack - 0xe0
|           ; var int64_t var_d8h @ stack - 0xd8
|           ; var int64_t var_d0h @ stack - 0xd0
|           ; var int64_t var_c8h @ stack - 0xc8
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_a8h @ stack - 0xa8
|           ; var int64_t var_98h @ stack - 0x98
|           ; var int64_t var_88h @ stack - 0x88
|           ; var int64_t var_80h @ stack - 0x80
|           ; var int64_t var_50h @ stack - 0x50
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_18h @ stack + 0x18
|           ; var int64_t var_20h @ stack + 0x20
|           0x14603b920      mov   qword [var_18h], rbx
|           0x14603b925      mov   qword [var_20h], rsi
|           0x14603b92a      push  rbp
|           0x14603b92b      push  rdi
|           0x14603b92c      push  r14
|           0x14603b92e      lea   rbp, qword [var_80h + 0x28]
|           0x14603b933      sub   rsp, 0x140
|           0x14603b93a      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x14603b941      xor   rax, rsp
|           0x14603b944      mov   qword [var_28h], rax
|           0x14603b948      mov   rsi, rdx                            ; arg2
|           0x14603b94b      mov   r14, rcx                            ; arg1
|           0x14603b94e      mov   qword [var_108h], rdx               ; arg2
|           0x14603b953      mov   rcx, qword [rdx+0x08]               ; arg2
|           0x14603b957      test  rcx, rcx
|       ,=< 0x14603b95a      jz    0x14603b964
|       |   0x14603b95c      lock  inc dword [rcx+0x08]
|       |   0x14603b960      mov   rcx, qword [rdx+0x08]               ; arg2
|       `-> 0x14603b964      mov   rax, qword [rdx]                    ; arg2
|           0x14603b967      mov   qword [r14+0x68], rax
|           0x14603b96b      mov   rbx, qword [r14+0x70]
|           0x14603b96f      mov   qword [r14+0x70], rcx
|           0x14603b973      mov   edi, 0xffffffff                     ; -1
|           0x14603b978      test  rbx, rbx
|       ,=< 0x14603b97b      jz    0x14603b9a6
|       |   0x14603b97d      mov   eax, edi
|       |   0x14603b97f      lock  xadd dword [rbx+0x08], eax
|       |   0x14603b984      cmp   eax, 0x01                           ; 1
|      ,==< 0x14603b987      jnz   0x14603b9a6
|      ||   0x14603b989      mov   rax, qword [rbx]
|      ||   0x14603b98c      mov   rcx, rbx
|      ||   0x14603b98f      call  qword [rax]
|      ||   0x14603b991      mov   eax, edi
|      ||   0x14603b993      lock  xadd dword [rbx+0x0c], eax
|      ||   0x14603b998      cmp   eax, 0x01                           ; 1
|     ,===< 0x14603b99b      jnz   0x14603b9a6
|     |||   0x14603b99d      mov   rax, qword [rbx]
|     |||   0x14603b9a0      mov   rcx, rbx
|     |||   0x14603b9a3      call  qword [rax+0x08]                    ; 8
|     ```-> 0x14603b9a6      cmp   byte [0x14d920598], 0x00            ; [0x14d920598:1]=0
|       ,=< 0x14603b9ad      jz    0x14603bae7
|       |   0x14603b9b3      mov   rdx, qword [rsi]
|       |   0x14603b9b6      test  rdx, rdx
|      ,==< 0x14603b9b9      jnz   0x14603ba40
|      ||   0x14603b9bf      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|      ||   0x14603b9c6      cmp   al, 0x06                            ; 6
|     ,===< 0x14603b9c8      jb    0x14603ba33
|     |||   0x14603b9ca      shr   rax, 0x08
|     |||   0x14603b9ce      cmp   al, 0x02                            ; 2
|    ,====< 0x14603b9d0      jb    0x14603ba33
|    ||||   0x14603b9d2      lea   rax, qword [0x148f72a30]            ; "[DFLog::RbxTransportClientLog] Connect called with empty connect configuration"
|    ||||   0x14603b9d9      mov   qword [var_118h], rax
|    ||||   0x14603b9de      mov   qword [var_110h], 0x4e              ; 'N'
|    ||||                                                              ; [0x4e:8]=-1 ; 78
|    ||||   0x14603b9e7      xor   eax, eax
|    ||||   0x14603b9e9      mov   qword [var_128h], rax
|    ||||   0x14603b9ee      lea   rax, qword [var_38h]
|    ||||   0x14603b9f2      mov   qword [var_120h], rax
|    ||||   0x14603b9f7      movaps xmm0, xmmword [var_128h]
|    ||||   0x14603b9fc      movdqa xmmword [var_128h], xmm0
|    ||||   0x14603ba02      movaps xmm1, xmmword [var_118h]
|    ||||   0x14603ba07      movdqa xmmword [var_118h], xmm1
|    ||||   0x14603ba0d      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|    ||||   0x14603ba14      movaps xmmword [var_c8h], xmm0
|    ||||   0x14603ba18      mov   byte [var_138h], 0x01
|    ||||   0x14603ba1d      lea   r9, qword [var_128h]
|    ||||   0x14603ba22      lea   r8, qword [var_118h]
|    ||||   0x14603ba27      mov   dl, 0x02
|    ||||   0x14603ba29      lea   rcx, qword [var_c8h]
|    ||||   0x14603ba2d      call  0x1438602b0
|    ||||   0x14603ba32      nop
|    ``---> 0x14603ba33      mov   rcx, rsi
|      ||   0x14603ba36      call  0x14073b9f0
|     ,===< 0x14603ba3b      jmp   0x14603bb44
|     |`--> 0x14603ba40      mov   rax, qword [r14]
|     | |   0x14603ba43      mov   rcx, r14
|     | |   0x14603ba46      call  qword [rax+0x08]                    ; 8
|     | |   0x14603ba49      test  al, al
|     |,==< 0x14603ba4b      jnz   0x14603bb14
|     |||   0x14603ba51      mov   rax, qword [0x14c421540]            ; [0x14c421540:8]=0x406
|     |||   0x14603ba58      cmp   al, 0x06                            ; 6
|    ,====< 0x14603ba5a      jb    0x14603bb14
|    ||||   0x14603ba60      shr   rax, 0x08
|    ||||   0x14603ba64      cmp   al, 0x02                            ; 2
|   ,=====< 0x14603ba66      jb    0x14603bb14
|   |||||   0x14603ba6c      mov   rax, qword [rsi]
|   |||||   0x14603ba6f      lea   rcx, qword [0x148f72a80]            ; "[DFLog::RbxTransportClientLog] Failed to start the BaseClient with configuration {}"
|   |||||   0x14603ba76      mov   qword [var_d8h], rcx
|   |||||   0x14603ba7a      mov   qword [var_d0h], 0x53               ; 'S' ; 83
|   |||||   0x14603ba82      mov   qword [var_f8h], rax
|   |||||   0x14603ba87      lea   rax, qword [0x1436a2f60]
|   |||||   0x14603ba8e      mov   qword [var_f0h], rax
|   |||||   0x14603ba93      movaps xmm0, xmmword [var_f8h]
|   |||||   0x14603ba98      movdqa xmmword [var_48h], xmm0
|   |||||   0x14603ba9d      mov   qword [var_e8h], 0x0f               ; [0xf:8]=-1 ; 15
|   |||||   0x14603baa6      lea   rax, qword [var_48h]
|   |||||   0x14603baaa      mov   qword [var_e0h], rax
|   |||||   0x14603baaf      movaps xmm0, xmmword [var_e8h]
|   |||||   0x14603bab4      movdqa xmmword [var_b8h], xmm0
|   |||||   0x14603bab9      movaps xmm1, xmmword [var_d8h]
|   |||||   0x14603babd      movdqa xmmword [var_a8h], xmm1
|   |||||   0x14603bac2      movups xmm0, xmmword [0x14c421540]        ; [0x14c421540:16]=-1
|   |||||   0x14603bac9      movaps xmmword [var_98h], xmm0
|   |||||   0x14603bacd      mov   byte [var_138h], 0x01
|   |||||   0x14603bad2      lea   r9, qword [var_b8h]
|   |||||   0x14603bad6      lea   r8, qword [var_a8h]
|   |||||   0x14603bada      mov   dl, 0x02
|   |||||   0x14603badc      lea   rcx, qword [var_98h]
|   |||||   0x14603bae0      call  0x1438602b0
|  ,======< 0x14603bae5      jmp   0x14603bb14
|  |||||`-> 0x14603bae7      xor   eax, eax
|  |||||    0x14603bae9      xchg  byte [r14+0x80], al
|  |||||    0x14603baf0      lea   rax, qword [0x148f731e0]
|  |||||    0x14603baf7      mov   qword [var_88h], rax
|  |||||    0x14603bafb      mov   qword [var_80h], r14
|  |||||    0x14603baff      lea   rax, qword [var_88h]
|  |||||    0x14603bb03      mov   qword [var_50h], rax
|  |||||    0x14603bb07      lea   rdx, qword [var_88h]
|  |||||    0x14603bb0b      mov   rcx, r14
|  |||||    0x14603bb0e      call  0x14603e7f0
|  |||||    0x14603bb13      nop
|  |||||    ; CODE XREF from fcn.14603b920 @ 0x14603bae5
|  ```-`--> 0x14603bb14      mov   rbx, qword [rsi+0x08]
|     |     0x14603bb18      test  rbx, rbx
|     | ,=< 0x14603bb1b      jz    0x14603bb44
|     | |   0x14603bb1d      mov   eax, edi
|     | |   0x14603bb1f      lock  xadd dword [rbx+0x08], eax
|     | |   0x14603bb24      cmp   eax, 0x01                           ; 1
|     |,==< 0x14603bb27      jnz   0x14603bb44
|     |||   0x14603bb29      mov   rax, qword [rbx]
|     |||   0x14603bb2c      mov   rcx, rbx
|     |||   0x14603bb2f      call  qword [rax]
|     |||   0x14603bb31      lock  xadd dword [rbx+0x0c], edi
|     |||   0x14603bb36      cmp   edi, 0x01                           ; 1
|    ,====< 0x14603bb39      jnz   0x14603bb44
|    ||||   0x14603bb3b      mov   rax, qword [rbx]
|    ||||   0x14603bb3e      mov   rcx, rbx
|    ||||   0x14603bb41      call  qword [rax+0x08]                    ; 8
|    ||||   ; CODE XREF from fcn.14603b920 @ 0x14603ba3b
|    ````-> 0x14603bb44      mov   rcx, qword [var_28h]
|           0x14603bb48      xor   rcx, rsp
|           0x14603bb4b      call  0x14730fca0
|           0x14603bb50      lea   r11, qword [var_28h + 0x10]
|           0x14603bb58      mov   rbx, qword [r11+0x30]
|           0x14603bb5c      mov   rsi, qword [r11+0x38]
|           0x14603bb60      mov   rsp, r11
|           0x14603bb63      pop   r14
|           0x14603bb65      pop   rdi
|           0x14603bb66      pop   rbp
\           0x14603bb67      ret
