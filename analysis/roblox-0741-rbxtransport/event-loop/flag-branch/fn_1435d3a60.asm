/ fcn.1435d3a60(int64_t arg1, int64_t arg2, int64_t arg3);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; arg int64_t arg3 @ r8
|           ; var int64_t var_48h @ stack - 0x48
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_20h @ stack + 0x20
|           0x1435d3a60      mov   qword [var_10h], rbx
|           0x1435d3a65      mov   qword [var_20h], rbp
|           0x1435d3a6a      push  rsi
|           0x1435d3a6b      push  rdi
|           0x1435d3a6c      push  r14
|           0x1435d3a6e      sub   rsp, 0x30
|           0x1435d3a72      mov   rbx, r8                             ; arg3
|           0x1435d3a75      mov   r14, rcx                            ; arg1
|           0x1435d3a78      mov   qword [var_28h], rcx                ; arg1
|           0x1435d3a7d      mov   qword [var_28h], rbx
|           0x1435d3a82      mov   qword [rcx], rdx                    ; arg2
|           0x1435d3a85      add   rcx, 0x08                           ; arg1
|           0x1435d3a89      lea   rdx, qword [0x148aa7760]            ; "SharedConnectionRegistry"
|           0x1435d3a90      call  0x1427c81e0
|           0x1435d3a95      call  0x1427c7e70
|           0x1435d3a9a      mov   qword [r14+0x18], rax
|           0x1435d3a9e      xor   ebp, ebp
|           0x1435d3aa0      mov   qword [r14+0x20], rbp
|           0x1435d3aa4      mov   qword [r14+0x28], rbp
|           0x1435d3aa8      mov   qword [r14+0x30], rbp
|           0x1435d3aac      mov   dword [r14+0x38], ebp
|           0x1435d3ab0      mov   qword [r14+0x58], rbp
|           0x1435d3ab4      mov   qword [r14+0x78], rbp
|           0x1435d3ab8      mov   rcx, qword [rbx+0x18]
|           0x1435d3abc      mov   qword [r14+0x78], rcx
|           0x1435d3ac0      mov   rdi, qword [rbx+0x18]
|           0x1435d3ac4      test  rdi, 0xfffffffffffffff8
|       ,=< 0x1435d3acb      jz    0x1435d3b24
|       |   0x1435d3acd      movzx eax, cl
|       |   0x1435d3ad0      sar   al, 0x01
|       |   0x1435d3ad2      test  al, 0x01                            ; 1
|      ,==< 0x1435d3ad4      jz    0x1435d3ae6
|      ||   0x1435d3ad6      and   rcx, 0xfffffffffffffffc
|      ||   0x1435d3ada      movzx eax, cl
|      ||   0x1435d3add      sar   al, 0x02
|      ||   0x1435d3ae0      not   al
|      ||   0x1435d3ae2      test  al, 0x01                            ; 1
|     ,===< 0x1435d3ae4      jz    0x1435d3afb
|     |`--> 0x1435d3ae6      movups xmm0, xmmword [rbx]
|     | |   0x1435d3ae9      movups xmmword [r14+0x60], xmm0
|     | |   0x1435d3aee      movsd xmm1, qword [rbx+0x10]
|     | |   0x1435d3af3      movsd qword [r14+0x70], xmm1
|     |,==< 0x1435d3af9      jmp   0x1435d3b1d
|     `---> 0x1435d3afb      and   rcx, 0xfffffffffffffff8
|      ||   0x1435d3aff      mov   rax, qword [rcx+0x08]
|      ||   0x1435d3b03      mov   rdx, rbx
|      ||   0x1435d3b06      lea   rcx, qword [r14+0x60]
|      ||   0x1435d3b0a      call  rax
|      ||   0x1435d3b0c      mov   rax, qword [r14+0x78]
|      ||   0x1435d3b10      and   rax, 0xfffffffffffffff8
|      ||   0x1435d3b14      mov   rdx, qword [rax+0x10]
|      ||   0x1435d3b18      mov   rcx, rbx
|      ||   0x1435d3b1b      call  rdx
|      ||   ; CODE XREF from fcn.1435d3a60 @ 0x1435d3af9
|      `--> 0x1435d3b1d      mov   qword [rbx+0x18], rbp
|       |   0x1435d3b21      mov   rdi, rbp
|       `-> 0x1435d3b24      mov   rax, rdi
|           0x1435d3b27      and   rax, 0xfffffffffffffffc
|           0x1435d3b2b      mov   rcx, rax
|           0x1435d3b2e      and   rcx, 0xfffffffffffffff8
|       ,=< 0x1435d3b32      jz    0x1435d3b63
|       |   0x1435d3b34      sar   dil, 0x01
|       |   0x1435d3b37      and   dil, 0x01
|       |   0x1435d3b3b      sar   al, 0x02
|       |   0x1435d3b3e      not   al
|       |   0x1435d3b40      test  al, 0x01                            ; 1
|      ,==< 0x1435d3b42      jnz   0x1435d3b55
|      ||   0x1435d3b44      mov   rax, qword [rcx+0x10]
|      ||   0x1435d3b48      test  dil, dil
|      ||   0x1435d3b4b      mov   rcx, rbx
|     ,===< 0x1435d3b4e      jnz   0x1435d3b53
|     |||   0x1435d3b50      mov   rcx, qword [rbx]
|     `---> 0x1435d3b53      call  rax
|      `--> 0x1435d3b55      test  dil, dil
|      ,==< 0x1435d3b58      jnz   0x1435d3b63
|      ||   0x1435d3b5a      mov   rcx, qword [rbx]
|      ||   0x1435d3b5d      call  0x14385ca60
|      ||   0x1435d3b62      nop
|      ``-> 0x1435d3b63      mov   rax, r14
|           0x1435d3b66      mov   rbx, qword [var_10h]
|           0x1435d3b6b      mov   rbp, qword [var_20h]
|           0x1435d3b70      add   rsp, 0x30
|           0x1435d3b74      pop   r14
|           0x1435d3b76      pop   rdi
|           0x1435d3b77      pop   rsi
\           0x1435d3b78      ret
