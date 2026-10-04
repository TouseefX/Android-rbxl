/ fcn.140007890(int64_t arg1);
|           ; arg int64_t arg1 @ rcx
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_30h @ stack - 0x30
|           ; var int64_t var_20h @ stack - 0x20
|           ; var int64_t var_18h @ stack - 0x18
|           0x140007890      push  rbp
|           0x140007891      push  rsi
|           0x140007892      push  rbx
|           0x140007893      sub   rsp, 0x40
|           0x140007897      lea   rbp, qword [var_18h]
|           0x14000789c      mov   ebx, ecx                            ; arg1
|           0x14000789e      call  fcn.140001b80
|           0x1400078a3      call  fcn.140001b80
|           0x1400078a8      mov   rcx, qword [rax]
|           0x1400078ab      mov   rax, qword [rcx+0x20]
|           0x1400078af      and   rax, 0xfffffffffffffffe
|       ,=< 0x1400078b3      jz    0x1400078e5
|       |   0x1400078b5      mov   rax, qword [rax+0x28]
|       |   0x1400078b9      shr   al, 0x04
|       |   0x1400078bc      mov   rdx, qword [0x14c39fe98]            ; [0x14c39fe98:8]=0x1428bd120
|       |   0x1400078c3      test  rdx, rdx
|       |   0x1400078c6      setnz r8b
|       |   0x1400078ca      and   r8b, al
|       |   0x1400078cd      cmp   r8b, 0x01                           ; 1
|      ,==< 0x1400078d1      jnz   0x1400078e5
|      ||   0x1400078d3      lea   rax, qword [0x1484aeee7]            ; "257f718-Kernel,1996"
|      ||   0x1400078da      mov   rsi, rcx
|      ||   0x1400078dd      mov   rcx, rax
|      ||   0x1400078e0      call  rdx
|      ||   0x1400078e2      mov   rcx, rsi
|      ``-> 0x1400078e5      mov   rdx, qword [rcx+0x70]
|           0x1400078e9      and   rdx, 0xffffffffffffffc0
|           0x1400078ed      movzx eax, bl
|           0x1400078f0      mov   qword [var_20h], rdx
|           0x1400078f4      mov   dword [var_38h], eax
|           0x1400078f8      mov   qword [var_30h], 0x00
|           0x140007901      lea   r8, qword [var_20h]
|           0x140007905      xor   r9d, r9d
|           0x140007908      call  fcn.1400109a0
|           0x14000790d      nop
|           0x14000790e      add   rsp, 0x40
|           0x140007912      pop   rbx
|           0x140007913      pop   rsi
|           0x140007914      pop   rbp
\           0x140007915      ret
