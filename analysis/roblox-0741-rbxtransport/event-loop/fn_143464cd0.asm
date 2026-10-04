/ fcn.143464cd0(int64_t arg1, int64_t arg2);
|           ; arg int64_t arg1 @ rcx
|           ; arg int64_t arg2 @ rdx
|           ; var int64_t var_108h @ stack - 0x108
|           ; var int64_t var_f8h @ stack - 0xf8
|           ; var int64_t var_f0h @ stack - 0xf0
|           ; var int64_t var_e8h @ stack - 0xe8
|           ; var int64_t var_e4h @ stack - 0xe4
|           ; var int64_t var_e0h @ stack - 0xe0
|           ; var int64_t var_d8h @ stack - 0xd8
|           ; var int64_t var_d0h @ stack - 0xd0
|           ; var int64_t var_c8h @ stack - 0xc8
|           ; var int64_t var_c0h @ stack - 0xc0
|           ; var int64_t var_b8h @ stack - 0xb8
|           ; var int64_t var_38h @ stack - 0x38
|           ; var int64_t var_28h @ stack - 0x28
|           ; var int64_t var_10h @ stack + 0x10
|           ; var int64_t var_18h @ stack + 0x18
|           ; var int64_t var_20h @ stack + 0x20
|           0x143464cd0      mov   qword [var_18h], rbp
|           0x143464cd5      mov   qword [var_20h], rsi
|           0x143464cda      push  rdi
|           0x143464cdb      push  r12
|           0x143464cdd      push  r13
|           0x143464cdf      push  r14
|           0x143464ce1      push  r15
|           0x143464ce3      sub   rsp, 0x100
|           0x143464cea      mov   rax, qword [0x14c4644b8]            ; [0x14c4644b8:8]=0x2b992ddfa232
|           0x143464cf1      xor   rax, rsp
|           0x143464cf4      mov   qword [var_38h], rax
|           0x143464cfc      movzx eax, byte [rcx+0x20]                ; arg1
|           0x143464d00      xor   ebp, ebp
|           0x143464d02      mov   qword [var_e8h], rbp
|           0x143464d07      nop
|           0x143464d08      mov   r8, rdx                             ; arg2
|           0x143464d0b      mov   r12, rcx                            ; arg1
|           0x143464d0e      test  al, al
|       ,=< 0x143464d10      jnz   0x143464d44
|       |   0x143464d12      test  rdx, rdx                            ; arg2
|      ,==< 0x143464d15      jle   0x143464d44
|      ||   0x143464d17      mov   rax, 0x12e0be826d694b2f
|      ||   0x143464d21      mov   rcx, r8
|      ||   0x143464d24      mul   r8
|      ||   0x143464d27      sub   rcx, rdx                            ; arg2
|      ||   0x143464d2a      shr   rcx, 0x01
|      ||   0x143464d2d      add   rcx, rdx                            ; arg2
|      ||   0x143464d30      shr   rcx, 0x1d
|      ||   0x143464d34      imul  rax, rcx, 0x3b9aca00
|      ||   0x143464d3b      cmp   r8, rax
|     ,===< 0x143464d3e      jnl   0x143464d49
|     |||   0x143464d40      dec   ecx
|    ,====< 0x143464d42      jmp   0x143464d49
|    ||``-> 0x143464d44      mov   r8, rbp
|    ||     0x143464d47      mov   ecx, ebp
|    ||     ; CODE XREF from fcn.143464cd0 @ 0x143464d42
|    ``---> 0x143464d49      mov   rax, 0x20c49ba5e353f7cf
|           0x143464d53      mov   dword [var_e8h], ecx
|           0x143464d57      imul  r8
|           0x143464d5a      movsxd r9, ecx
|           0x143464d5d      sar   rdx, 0x07                           ; arg2
|           0x143464d61      mov   rax, rdx                            ; arg2
|           0x143464d64      shr   rax, 0x3f
|           0x143464d68      add   rdx, rax                            ; arg2
|           0x143464d6b      imul  rax, rdx, 0x3e8
|           0x143464d72      cmp   r8, rax
|       ,=< 0x143464d75      jnl   0x143464d7a
|       |   0x143464d77      dec   rdx                                 ; arg2
|       `-> 0x143464d7a      imul  rax, r9, 0xf4240
|           0x143464d81      lea   rsi, qword [r12+0x290]
|           0x143464d89      sub   rdx, rax                            ; arg2
|           0x143464d8c      mov   rax, qword [r12+0x298]
|           0x143464d94      mov   qword [r12+0x2a0], rax
|           0x143464d9c      mov   rax, qword [rsi+0x18]
|           0x143464da0      sub   rax, qword [rsi+0x08]
|           0x143464da4      mov   dword [var_e4h], edx                ; arg2
|           0x143464da8      mov   edx, dword [r12+0x30]
|           0x143464dad      sar   rax, 0x03
|           0x143464db1      add   edx, 0x02
|           0x143464db4      cmp   rdx, rax
|       ,=< 0x143464db7      jbe   0x143464dc1
|       |   0x143464db9      mov   rcx, rsi
|       |   0x143464dbc      call  0x1427107f0
|       `-> 0x143464dc1      mov   rdx, qword [r12+0x2a0]
|           0x143464dc9      mov   qword [var_f0h], rbp
|           0x143464dce      cmp   rdx, qword [r12+0x2a8]
|       ,=< 0x143464dd6      jz    0x143464de2
|       |   0x143464dd8      mov   qword [rdx], rbp
|       |   0x143464ddb      add   qword [rsi+0x10], 0x08
|      ,==< 0x143464de0      jmp   0x143464def
|      |`-> 0x143464de2      lea   r8, qword [var_f0h]
|      |    0x143464de7      mov   rcx, rsi
|      |    0x143464dea      call  0x14099cee0
|      |    ; CODE XREF from fcn.143464cd0 @ 0x143464de0
|      `--> 0x143464def      mov   rdx, qword [rsi+0x10]
|           0x143464df3      lea   rdi, qword [r12+0x08]
|           0x143464df8      cmp   rdx, qword [r12+0x2a8]
|       ,=< 0x143464e00      jz    0x143464e0f
|       |   0x143464e02      mov   rax, qword [rdi]
|       |   0x143464e05      mov   qword [rdx], rax
|       |   0x143464e08      add   qword [rsi+0x10], 0x08
|      ,==< 0x143464e0d      jmp   0x143464e1a
|      |`-> 0x143464e0f      mov   r8, rdi
|      |    0x143464e12      mov   rcx, rsi
|      |    0x143464e15      call  0x14099cee0
|      |    ; CODE XREF from fcn.143464cd0 @ 0x143464e0d
|      `--> 0x143464e1a      mov   rax, qword [r12+0x4c8]
|           0x143464e22      lea   r15, qword [r12+0x4c0]
|           0x143464e2a      mov   qword [r12+0x4d0], rax
|           0x143464e32      mov   edx, dword [r12+0x30]
|           0x143464e37      mov   rax, qword [r15+0x18]
|           0x143464e3b      inc   edx
|           0x143464e3d      sub   rax, qword [r15+0x08]
|           0x143464e41      sar   rax, 0x03
|           0x143464e45      cmp   rdx, rax
|       ,=< 0x143464e48      jbe   0x143464e52
|       |   0x143464e4a      mov   rcx, r15
|       |   0x143464e4d      call  0x1427107f0
|       `-> 0x143464e52      mov   rdx, qword [r12+0x4d0]
|           0x143464e5a      mov   qword [var_c0h], rbp
|           0x143464e5f      cmp   rdx, qword [r12+0x4d8]
|       ,=< 0x143464e67      jz    0x143464e73
|       |   0x143464e69      mov   qword [rdx], rbp
|       |   0x143464e6c      add   qword [r15+0x10], 0x08
|      ,==< 0x143464e71      jmp   0x143464e80
|      |`-> 0x143464e73      lea   r8, qword [var_c0h]
|      |    0x143464e78      mov   rcx, r15
|      |    0x143464e7b      call  0x14099cee0
|      |    ; CODE XREF from fcn.143464cd0 @ 0x143464e71
|      `--> 0x143464e80      mov   rax, qword [r12+0x6f8]
|           0x143464e88      lea   r13, qword [r12+0x6f0]
|           0x143464e90      mov   edx, dword [r12+0x30]
|           0x143464e95      mov   qword [r12+0x700], rax
|           0x143464e9d      inc   edx
|           0x143464e9f      mov   rax, qword [r13+0x18]
|           0x143464ea3      sub   rax, qword [r13+0x08]
|           0x143464ea7      sar   rax, 0x03
|           0x143464eab      cmp   rdx, rax
|       ,=< 0x143464eae      jbe   0x143464eb8
|       |   0x143464eb0      mov   rcx, r13
|       |   0x143464eb3      call  0x1427107f0
|       `-> 0x143464eb8      mov   rdx, qword [r12+0x700]
|           0x143464ec0      mov   qword [var_e0h], rbp
|           0x143464ec5      cmp   rdx, qword [r12+0x708]
|       ,=< 0x143464ecd      jz    0x143464ed9
|       |   0x143464ecf      mov   qword [rdx], rbp
|       |   0x143464ed2      add   qword [r13+0x10], 0x08
|      ,==< 0x143464ed7      jmp   0x143464ee6
|      |`-> 0x143464ed9      lea   r8, qword [var_e0h]
|      |    0x143464ede      mov   rcx, r13
|      |    0x143464ee1      call  0x14099cee0
|      |    ; CODE XREF from fcn.143464cd0 @ 0x143464ed7
|      `--> 0x143464ee6      lea   r14, qword [r12+0x28]
|           0x143464eeb      mov   qword [var_10h], rbx
|           0x143464ef3      cmp   dword [r14+0x08], ebp
|       ,=< 0x143464ef7      jz    0x143465002
|       |   0x143464efd      mov   rcx, qword [r14]
|       |   0x143464f00      mov   edi, dword [r14+0x10]
|       |   0x143464f04      mov   rbx, rcx
|       |   0x143464f07      shl   rdi, 0x04
|       |   0x143464f0b      add   rdi, rcx
|       |   0x143464f0e      cmp   rcx, rdi
|      ,==< 0x143464f11      jz    0x143464f22
|     .---> 0x143464f13      cmp   qword [rbx], 0xfffffffffffffffe
|    ,====< 0x143464f17      jb    0x143464f22
|    |:||   0x143464f19      add   rbx, 0x10                           ; 16
|    |:||   0x143464f1d      cmp   rbx, rdi
|    |`===< 0x143464f20      jnz   0x143464f13
|    `-`--> 0x143464f22      mov   eax, dword [r14+0x10]
|       |   0x143464f26      shl   rax, 0x04
|       |   0x143464f2a      add   rax, rcx
|       |   0x143464f2d      cmp   rbx, rax
|      ,==< 0x143464f30      jz    0x143464ffd
|      ||   0x143464f36      mov   r14, rax
|      ||   0x143464f39      nop   dword [rax], eax
|     .---> 0x143464f40      mov   rax, qword [rbx+0x08]
|     :||   0x143464f44      test  byte [rax+0x1c], 0x01
|    ,====< 0x143464f48      jz    0x143464f74
|    |:||   0x143464f4a      mov   rdx, qword [r12+0x2a0]
|    |:||   0x143464f52      cmp   rdx, qword [r12+0x2a8]
|   ,=====< 0x143464f5a      jz    0x143464f69
|   ||:||   0x143464f5c      mov   rax, qword [rbx]
|   ||:||   0x143464f5f      mov   qword [rdx], rax
|   ||:||   0x143464f62      add   qword [rsi+0x10], 0x08
|  ,======< 0x143464f67      jmp   0x143464f74
|  |`-----> 0x143464f69      mov   r8, rbx
|  | |:||   0x143464f6c      mov   rcx, rsi
|  | |:||   0x143464f6f      call  0x14099cee0
|  | |:||   ; CODE XREF from fcn.143464cd0 @ 0x143464f67
|  `-`----> 0x143464f74      mov   rax, qword [rbx+0x08]
|     :||   0x143464f78      test  byte [rax+0x1c], 0x02
|    ,====< 0x143464f7c      jz    0x143464fa8
|    |:||   0x143464f7e      mov   rdx, qword [r12+0x4d0]
|    |:||   0x143464f86      cmp   rdx, qword [r12+0x4d8]
|   ,=====< 0x143464f8e      jz    0x143464f9d
|   ||:||   0x143464f90      mov   rax, qword [rbx]
|   ||:||   0x143464f93      mov   qword [rdx], rax
|   ||:||   0x143464f96      add   qword [r15+0x10], 0x08
|  ,======< 0x143464f9b      jmp   0x143464fa8
|  |`-----> 0x143464f9d      mov   r8, rbx
|  | |:||   0x143464fa0      mov   rcx, r15
|  | |:||   0x143464fa3      call  0x14099cee0
|  | |:||   ; CODE XREF from fcn.143464cd0 @ 0x143464f9b
|  `-`----> 0x143464fa8      mov   rdx, qword [r12+0x700]
|     :||   0x143464fb0      cmp   rdx, qword [r12+0x708]
|    ,====< 0x143464fb8      jz    0x143464fc7
|    |:||   0x143464fba      mov   rax, qword [rbx]
|    |:||   0x143464fbd      mov   qword [rdx], rax
|    |:||   0x143464fc0      add   qword [r13+0x10], 0x08
|   ,=====< 0x143464fc5      jmp   0x143464fd2
|   |`----> 0x143464fc7      mov   r8, rbx
|   | :||   0x143464fca      mov   rcx, r13
|   | :||   0x143464fcd      call  0x14099cee0
|   | :||   ; CODE XREF from fcn.143464cd0 @ 0x143464fc5
|   `-----> 0x143464fd2      add   rbx, 0x10                           ; 16
|     :||   0x143464fd6      cmp   rbx, rdi
|    ,====< 0x143464fd9      jz    0x143464fef
|    |:||   0x143464fdb      nop   dword [rax+rax*1], eax
|   .-----> 0x143464fe0      cmp   qword [rbx], 0xfffffffffffffffe
|  ,======< 0x143464fe4      jb    0x143464fef
|  |:|:||   0x143464fe6      add   rbx, 0x10                           ; 16
|  |:|:||   0x143464fea      cmp   rbx, rdi
|  |`=====< 0x143464fed      jnz   0x143464fe0
|  `-`----> 0x143464fef      cmp   rbx, r14
|     `===< 0x143464ff2      jnz   0x143464f40
|      ||   0x143464ff8      lea   r14, qword [r12+0x28]
|      `--> 0x143464ffd      lea   rdi, qword [r12+0x08]
|       `-> 0x143465002      mov   rcx, qword [r12+0x298]
|           0x14346500a      mov   rdx, rbp
|           0x14346500d      mov   rax, qword [r12+0x2a0]
|           0x143465015      mov   r9, rbp
|           0x143465018      sub   rax, rcx
|           0x14346501b      mov   r8, rbp
|           0x14346501e      sar   rax, 0x03
|           0x143465022      dec   rax
|           0x143465025      mov   qword [rcx], rax
|           0x143465028      mov   rcx, qword [r12+0x4c8]
|           0x143465030      mov   rax, qword [r12+0x4d0]
|           0x143465038      sub   rax, rcx
|           0x14346503b      sar   rax, 0x03
|           0x14346503f      dec   rax
|           0x143465042      mov   qword [rcx], rax
|           0x143465045      mov   rcx, qword [r12+0x6f8]
|           0x14346504d      mov   rax, qword [r12+0x700]
|           0x143465055      sub   rax, rcx
|           0x143465058      sar   rax, 0x03
|           0x14346505c      dec   rax
|           0x14346505f      mov   qword [rcx], rax
|           0x143465062      mov   rcx, qword [r12+0x6f8]
|           0x14346506a      mov   rax, qword [r12+0x4c8]
|           0x143465072      mov   r13, qword [r12+0x298]
|           0x14346507a      mov   qword [var_d8h], rax
|           0x14346507f      cmp   dword [rcx], ebp
|           0x143465081      mov   qword [var_e0h], rcx
|           0x143465086      cmovnbe r9, rcx
|           0x14346508a      cmp   dword [rax], ebp
|           0x14346508c      mov   ecx, 0xffffffff                     ; -1
|           0x143465091      cmovnbe r8, rax
|           0x143465095      cmp   dword [r13], edx
|           0x143465099      lea   rax, qword [var_e8h]
|           0x14346509e      cmovnbe rdx, r13
|           0x1434650a2      mov   qword [var_108h], rax
|           0x1434650a7      call  qword [sym.imp.WS2_32.dll_select]   ; [0x148441ef0:8]=0x8000000000000012 ; int select(int nfds, fd_set *readfds, fd_set *writefds, fd_set *exceptfds, struct timeval *timeout)
|           0x1434650ad      mov   dword [var_f0h], eax
|           0x1434650b1      mov   ecx, ebp
|           0x1434650b3      xchg  byte [r12+0x20], cl
|           0x1434650b8      cmp   eax, 0xffffffff                     ; -1
|       ,=< 0x1434650bb      jz    0x14346554c
|       |   0x1434650c1      mov   rdx, qword [r12+0x40]
|       |   0x1434650c6      lea   rbx, qword [r12+0x40]
|       |   0x1434650cb      mov   qword [rbx+0x08], rdx
|       |   0x1434650cf      mov   r15, rbp
|       |   0x1434650d2      mov   qword [rbx+0x18], rbp
|       |   0x1434650d6      cmp   dword [r13], ebp
|      ,==< 0x1434650da      jbe   0x1434651da
|      ||   0x1434650e0      lea   rsi, qword [r13+0x08]
|      ||   0x1434650e4      mov   r14, 0x2aaaaaaaaaaaaaab
|      ||   0x1434650ee      nop
|     .---> 0x1434650f0      mov   r9, qword [rsi]
|     :||   0x1434650f3      mov   rcx, qword [rdi]
|     :||   0x1434650f6      cmp   r9, rcx
|    ,====< 0x1434650f9      jnz   0x143465114
|    |:||   0x1434650fb      xor   r9d, r9d
|    |:||   0x1434650fe      lea   rdx, qword [var_b8h]
|    |:||   0x143465103      mov   r8d, 0x80                           ; 128
|    |:||   0x143465109      call  qword [sym.imp.WS2_32.dll_recv]     ; [0x148441ee8:8]=0x8000000000000010 ; int recv(SOCKET s, char *buf, int len, int flags)
|   ,=====< 0x14346510f      jmp   0x1434651bd
|   |`----> 0x143465114      mov   r8, qword [rbx+0x08]
|   | :||   0x143465118      mov   rax, r14
|   | :||   0x14346511b      mov   rcx, r8
|   | :||   0x14346511e      sub   rcx, qword [rbx]
|   | :||   0x143465121      imul  rcx
|   | :||   0x143465124      sar   rdx, 0x02
|   | :||   0x143465128      mov   rax, rdx
|   | :||   0x14346512b      shr   rax, 0x3f
|   | :||   0x14346512f      add   rdx, rax
|   | :||   0x143465132      lea   rax, qword [rdx+rdx*2]
|   | :||   0x143465136      shr   rax, 0x02
|   | :||   0x14346513a      cmp   qword [rbx+0x18], rax
|   |,====< 0x14346513e      jb    0x14346514f
|   ||:||   0x143465140      mov   rcx, rbx
|   ||:||   0x143465143      call  0x143465640
|   ||:||   0x143465148      mov   r8, qword [rbx+0x08]
|   ||:||   0x14346514c      mov   r9, qword [rsi]
|   |`----> 0x14346514f      mov   r11, qword [rbx]
|   | :||   0x143465152      mov   rax, r14
|   | :||   0x143465155      mov   rdi, qword [rbx+0x20]
|   | :||   0x143465159      sub   r8, r11
|   | :||   0x14346515c      imul  r8
|   | :||   0x14346515f      mov   rcx, r9
|   | :||   0x143465162      sar   rdx, 0x02
|   | :||   0x143465166      mov   r10, rdx
|   | :||   0x143465169      dec   rdx
|   | :||   0x14346516c      shr   r10, 0x3f
|   | :||   0x143465170      add   r10, rdx
|   | :||   0x143465173      mov   rdx, rbp
|   | :||   0x143465176      and   rcx, r10
|   | :||   0x143465179      nop   dword [rax], eax
|   |.----> 0x143465180      lea   rax, qword [rcx+rcx*2]
|   |::||   0x143465184      lea   r8, qword [r11+rax*8]
|   |::||   0x143465188      mov   rax, qword [r11+rax*8]
|   |::||   0x14346518c      cmp   rax, rdi
|  ,======< 0x14346518f      jz    0x1434651ac
|  ||::||   0x143465191      cmp   rax, r9
| ,=======< 0x143465194      jz    0x1434651b3
| |||::||   0x143465196      inc   rcx
| |||::||   0x143465199      add   rcx, rdx
| |||::||   0x14346519c      inc   rdx
| |||::||   0x14346519f      and   rcx, r10
| |||::||   0x1434651a2      cmp   rdx, r10
| |||`====< 0x1434651a5      jbe   0x143465180
| ||| :||   0x1434651a7      mov   r8, rbp
| |||,====< 0x1434651aa      jmp   0x1434651b3
| |`------> 0x1434651ac      mov   qword [r8], r9
| | ||:||   0x1434651af      inc   qword [rbx+0x18]
| | ||:||   ; CODE XREF from fcn.143464cd0 @ 0x1434651aa
| `--`----> 0x1434651b3      or    dword [r8+0x08], 0x01
|   | :||   0x1434651b8      lea   rdi, qword [r12+0x08]
|   | :||   ; CODE XREF from fcn.143464cd0 @ 0x14346510f
|   `-----> 0x1434651bd      mov   eax, dword [r13]
|     :||   0x1434651c1      inc   r15
|     :||   0x1434651c4      add   rsi, 0x08
|     :||   0x1434651c8      cmp   r15, rax
|     `===< 0x1434651cb      jb    0x1434650f0
|      ||   0x1434651d1      mov   rdx, qword [rbx+0x08]
|      ||   0x1434651d5      lea   r14, qword [r12+0x28]
|      `--> 0x1434651da      mov   r13, qword [var_d8h]
|       |   0x1434651df      mov   rsi, rbp
|       |   0x1434651e2      cmp   dword [r13], esi
|      ,==< 0x1434651e6      jbe   0x1434652c5
|      ||   0x1434651ec      lea   r15, qword [r13+0x08]
|      ||   0x1434651f0      mov   r14, 0x2aaaaaaaaaaaaaab
|      ||   0x1434651fa      nop   word [rax+rax*1], ax
|     .---> 0x143465200      mov   r8, qword [rbx+0x08]
|     :||   0x143465204      mov   rax, r14
|     :||   0x143465207      mov   r10, qword [rbx]
|     :||   0x14346520a      mov   rcx, r8
|     :||   0x14346520d      sub   rcx, r10
|     :||   0x143465210      imul  rcx
|     :||   0x143465213      sar   rdx, 0x02
|     :||   0x143465217      mov   rax, rdx
|     :||   0x14346521a      shr   rax, 0x3f
|     :||   0x14346521e      add   rdx, rax
|     :||   0x143465221      lea   rax, qword [rdx+rdx*2]
|     :||   0x143465225      shr   rax, 0x02
|     :||   0x143465229      cmp   qword [rbx+0x18], rax
|    ,====< 0x14346522d      jb    0x14346523e
|    |:||   0x14346522f      mov   rcx, rbx
|    |:||   0x143465232      call  0x143465640
|    |:||   0x143465237      mov   r8, qword [rbx+0x08]
|    |:||   0x14346523b      mov   r10, qword [rbx]
|    `----> 0x14346523e      mov   r11, qword [r15]
|     :||   0x143465241      sub   r8, r10
|     :||   0x143465244      mov   rdi, qword [rbx+0x20]
|     :||   0x143465248      mov   rax, r14
|     :||   0x14346524b      imul  r8
|     :||   0x14346524e      sar   rdx, 0x02
|     :||   0x143465252      mov   r9, rdx
|     :||   0x143465255      dec   rdx
|     :||   0x143465258      shr   r9, 0x3f
|     :||   0x14346525c      add   r9, rdx
|     :||   0x14346525f      mov   rdx, rbp
|     :||   0x143465262      mov   rcx, r9
|     :||   0x143465265      and   rcx, r11
|     :||   0x143465268      nop   dword [rax+rax*1], eax
|    .----> 0x143465270      lea   rax, qword [rcx+rcx*2]
|    ::||   0x143465274      lea   r8, qword [r10+rax*8]
|    ::||   0x143465278      mov   rax, qword [r10+rax*8]
|    ::||   0x14346527c      cmp   rax, rdi
|   ,=====< 0x14346527f      jz    0x14346529c
|   |::||   0x143465281      cmp   rax, r11
|  ,======< 0x143465284      jz    0x1434652a3
|  ||::||   0x143465286      inc   rcx
|  ||::||   0x143465289      add   rcx, rdx
|  ||::||   0x14346528c      inc   rdx
|  ||::||   0x14346528f      and   rcx, r9
|  ||::||   0x143465292      cmp   rdx, r9
|  ||`====< 0x143465295      jbe   0x143465270
|  || :||   0x143465297      mov   r8, rbp
|  ||,====< 0x14346529a      jmp   0x1434652a3
|  |`-----> 0x14346529c      mov   qword [r8], r11
|  | |:||   0x14346529f      inc   qword [rbx+0x18]
|  | |:||   ; CODE XREF from fcn.143464cd0 @ 0x14346529a
|  `-`----> 0x1434652a3      or    dword [r8+0x08], 0x02
|     :||   0x1434652a8      inc   rsi
|     :||   0x1434652ab      mov   eax, dword [r13]
|     :||   0x1434652af      add   r15, 0x08
|     :||   0x1434652b3      cmp   rsi, rax
|     `===< 0x1434652b6      jb    0x143465200
|      ||   0x1434652bc      mov   rdx, qword [rbx+0x08]
|      ||   0x1434652c0      lea   r14, qword [r12+0x28]
|      `--> 0x1434652c5      mov   r13, qword [var_e0h]
|       |   0x1434652ca      mov   rsi, rbp
|       |   0x1434652cd      cmp   dword [r13], esi
|      ,==< 0x1434652d1      jbe   0x1434653b5
|      ||   0x1434652d7      lea   r15, qword [r13+0x08]
|      ||   0x1434652db      mov   r14, 0x2aaaaaaaaaaaaaab
|      ||   0x1434652e5      nop   word [rax+rax*1], ax
|     .---> 0x1434652f0      mov   r8, qword [rbx+0x08]
|     :||   0x1434652f4      mov   rax, r14
|     :||   0x1434652f7      mov   r10, qword [rbx]
|     :||   0x1434652fa      mov   rcx, r8
|     :||   0x1434652fd      sub   rcx, r10
|     :||   0x143465300      imul  rcx
|     :||   0x143465303      sar   rdx, 0x02
|     :||   0x143465307      mov   rax, rdx
|     :||   0x14346530a      shr   rax, 0x3f
|     :||   0x14346530e      add   rdx, rax
|     :||   0x143465311      lea   rax, qword [rdx+rdx*2]
|     :||   0x143465315      shr   rax, 0x02
|     :||   0x143465319      cmp   qword [rbx+0x18], rax
|    ,====< 0x14346531d      jb    0x14346532e
|    |:||   0x14346531f      mov   rcx, rbx
|    |:||   0x143465322      call  0x143465640
|    |:||   0x143465327      mov   r8, qword [rbx+0x08]
|    |:||   0x14346532b      mov   r10, qword [rbx]
|    `----> 0x14346532e      mov   r11, qword [r15]
|     :||   0x143465331      sub   r8, r10
|     :||   0x143465334      mov   rdi, qword [rbx+0x20]
|     :||   0x143465338      mov   rax, r14
|     :||   0x14346533b      imul  r8
|     :||   0x14346533e      sar   rdx, 0x02
|     :||   0x143465342      mov   r9, rdx
|     :||   0x143465345      dec   rdx
|     :||   0x143465348      shr   r9, 0x3f
|     :||   0x14346534c      add   r9, rdx
|     :||   0x14346534f      mov   rdx, rbp
|     :||   0x143465352      mov   rcx, r9
|     :||   0x143465355      and   rcx, r11
|     :||   0x143465358      nop   dword [rax+rax*1], eax
|    .----> 0x143465360      lea   rax, qword [rcx+rcx*2]
|    ::||   0x143465364      lea   r8, qword [r10+rax*8]
|    ::||   0x143465368      mov   rax, qword [r10+rax*8]
|    ::||   0x14346536c      cmp   rax, rdi
|   ,=====< 0x14346536f      jz    0x14346538c
|   |::||   0x143465371      cmp   rax, r11
|  ,======< 0x143465374      jz    0x143465393
|  ||::||   0x143465376      inc   rcx
|  ||::||   0x143465379      add   rcx, rdx
|  ||::||   0x14346537c      inc   rdx
|  ||::||   0x14346537f      and   rcx, r9
|  ||::||   0x143465382      cmp   rdx, r9
|  ||`====< 0x143465385      jbe   0x143465360
|  || :||   0x143465387      mov   r8, rbp
|  ||,====< 0x14346538a      jmp   0x143465393
|  |`-----> 0x14346538c      mov   qword [r8], r11
|  | |:||   0x14346538f      inc   qword [rbx+0x18]
|  | |:||   ; CODE XREF from fcn.143464cd0 @ 0x14346538a
|  `-`----> 0x143465393      or    dword [r8+0x08], 0x08
|     :||   0x143465398      inc   rsi
|     :||   0x14346539b      mov   eax, dword [r13]
|     :||   0x14346539f      add   r15, 0x08
|     :||   0x1434653a3      cmp   rsi, rax
|     `===< 0x1434653a6      jb    0x1434652f0
|      ||   0x1434653ac      mov   rdx, qword [rbx+0x08]
|      ||   0x1434653b0      lea   r14, qword [r12+0x28]
|      `--> 0x1434653b5      mov   rcx, qword [rbx]
|       |   0x1434653b8      mov   r9, 0x2aaaaaaaaaaaaaab
|       |   0x1434653c2      sub   rdx, rcx
|       |   0x1434653c5      mov   rax, r9
|       |   0x1434653c8      imul  rdx
|       |   0x1434653cb      mov   rdi, rbp
|       |   0x1434653ce      mov   r15, rdx
|       |   0x1434653d1      sar   r15, 0x02
|       |   0x1434653d5      mov   rax, r15
|       |   0x1434653d8      shr   rax, 0x3f
|       |   0x1434653dc      add   r15, rax
|      ,==< 0x1434653df      jz    0x1434653f6
|      ||   0x1434653e1      mov   rax, qword [rbx+0x20]
|     .---> 0x1434653e5      cmp   qword [rcx], rax
|    ,====< 0x1434653e8      jnz   0x1434653f6
|    |:||   0x1434653ea      inc   rdi
|    |:||   0x1434653ed      add   rcx, 0x18                           ; 24
|    |:||   0x1434653f1      cmp   rdi, r15
|    |`===< 0x1434653f4      jb    0x1434653e5
|    | ||   ; CODE XREF from fcn.143464cd0 @ 0x1434654f9
|   .`.`--> 0x1434653f6      cmp   rdi, r15
|   : :,==< 0x1434653f9      jz    0x1434654fe
|   : :||   0x1434653ff      mov   rsi, qword [rbx]
|   : :||   0x143465402      lea   rax, qword [rdi+rdi*2]
|   : :||   0x143465406      lea   r13, qword [rax*8]
|   : :||   0x14346540e      mov   rcx, r14
|   : :||   0x143465411      add   rsi, r13
|   : :||   0x143465414      lea   r8, qword [var_c8h]
|   : :||   0x143465419      mov   rdx, rsi
|   : :||   0x14346541c      call  0x1408dd540
|   : :||   0x143465421      mov   rdx, qword [var_c8h]
|   : :||   0x143465426      test  al, al
|   :,====< 0x143465428      jnz   0x14346549e
|   :|:||   0x14346542a      mov   r9d, dword [r14+0x08]
|   :|:||   0x14346542e      mov   r8d, dword [r14+0x10]
|   :|:||   0x143465432      inc   r9d
|   :|:||   0x143465435      mov   qword [var_f8h], rdx
|   :|:||   0x14346543a      lea   eax, qword [r9*4]
|   :|:||   0x143465442      lea   ecx, qword [r8+r8*2]
|   :|:||   0x143465446      cmp   eax, ecx
|  ,======< 0x143465448      jb    0x143465450
|  |:|:||   0x14346544a      lea   edx, qword [r8+r8*1]
| ,=======< 0x14346544e      jmp   0x143465467
| |`------> 0x143465450      mov   ecx, r8d
| | :|:||   0x143465453      mov   eax, r8d
| | :|:||   0x143465456      sub   ecx, dword [r14+0x0c]
| | :|:||   0x14346545a      sub   ecx, r9d
| | :|:||   0x14346545d      shr   eax, 0x03
| | :|:||   0x143465460      cmp   ecx, eax
| |,======< 0x143465462      jnbe  0x143465484
| ||:|:||   0x143465464      mov   edx, r8d
| ||:|:||   ; CODE XREF from fcn.143464cd0 @ 0x14346544e
| `-------> 0x143465467      mov   rcx, r14
|  |:|:||   0x14346546a      call  0x140d9c2a0
|  |:|:||   0x14346546f      lea   r8, qword [var_f8h]
|  |:|:||   0x143465474      mov   rdx, rsi
|  |:|:||   0x143465477      mov   rcx, r14
|  |:|:||   0x14346547a      call  0x1408dd540
|  |:|:||   0x14346547f      mov   rdx, qword [var_f8h]
|  `------> 0x143465484      inc   dword [r14+0x08]
|   :|:||   0x143465488      cmp   qword [rdx], 0xffffffffffffffff
|  ,======< 0x14346548c      jz    0x143465492
|  |:|:||   0x14346548e      dec   dword [r14+0x0c]
|  `------> 0x143465492      mov   rax, qword [rsi]
|   :|:||   0x143465495      mov   qword [rdx], rax
|   :|:||   0x143465498      xor   eax, eax
|   :|:||   0x14346549a      mov   qword [rdx+0x08], rax
|   :`----> 0x14346549e      mov   rax, qword [rdx+0x08]
|   : :||   0x1434654a2      mov   r9, 0x2aaaaaaaaaaaaaab
|   : :||   0x1434654ac      mov   qword [rsi+0x10], rax
|   : :||   0x1434654b0      mov   rax, r9
|   : :||   0x1434654b3      mov   r8, qword [rbx]
|   : :||   0x1434654b6      mov   rcx, qword [rbx+0x08]
|   : :||   0x1434654ba      sub   rcx, r8
|   : :||   0x1434654bd      imul  rcx
|   : :||   0x1434654c0      lea   rcx, qword [r8+r13*1]
|   : :||   0x1434654c4      sar   rdx, 0x02
|   : :||   0x1434654c8      mov   rax, rdx
|   : :||   0x1434654cb      shr   rax, 0x3f
|   : :||   0x1434654cf      add   rdx, rax
|   : :||   0x1434654d2      nop   dword [rax], eax
|   : :||   0x1434654d6      nop   word [rax+rax*1], ax
|   :.----> 0x1434654e0      inc   rdi
|   :::||   0x1434654e3      lea   rcx, qword [rcx+0x18]
|   :::||   0x1434654e7      cmp   rdi, rdx
|   `=====< 0x1434654ea      jnb   0x1434653f6
|    ::||   0x1434654f0      mov   rax, qword [rbx+0x20]
|    ::||   0x1434654f4      cmp   qword [rcx], rax
|    `====< 0x1434654f7      jz    0x1434654e0
|     `===< 0x1434654f9      jmp   0x1434653f6
|      `--> 0x1434654fe      mov   r8, qword [rbx]
|       |   0x143465501      mov   rax, r9
|       |   0x143465504      mov   rcx, qword [rbx+0x08]
|       |   0x143465508      sub   rcx, r8
|       |   0x14346550b      imul  rcx
|       |   0x14346550e      sar   rdx, 0x02
|       |   0x143465512      mov   rax, rdx
|       |   0x143465515      shr   rax, 0x3f
|       |   0x143465519      add   rdx, rax
|      ,==< 0x14346551c      jz    0x143465533
|      ||   0x14346551e      mov   rax, qword [rbx+0x20]
|     .---> 0x143465522      cmp   qword [r8], rax
|    ,====< 0x143465525      jnz   0x143465533
|    |:||   0x143465527      inc   rbp
|    |:||   0x14346552a      add   r8, 0x18                            ; 24
|    |:||   0x14346552e      cmp   rbp, rdx
|    |`===< 0x143465531      jb    0x143465522
|    `-`--> 0x143465533      mov   eax, dword [var_f0h]
|       |   0x143465537      mov   qword [var_d8h], rbx
|       |   0x14346553c      mov   qword [var_d0h], rbp
|       |   0x143465541      movups xmm0, xmmword [var_d8h]
|       |   0x143465546      movups xmmword [r12+0x70], xmm0
|       `-> 0x14346554c      mov   rbx, qword [var_10h]
|           0x143465554      mov   rcx, qword [var_38h]
|           0x14346555c      xor   rcx, rsp
|           0x14346555f      call  fcn.14730fca0
|           0x143465564      lea   r11, qword [var_28h]
|           0x14346556c      mov   rbp, qword [r11+0x40]
|           0x143465570      mov   rsi, qword [r11+0x48]
|           0x143465574      mov   rsp, r11
|           0x143465577      pop   r15
|           0x143465579      pop   r14
|           0x14346557b      pop   r13
|           0x14346557d      pop   r12
|           0x14346557f      pop   rdi
\           0x143465580      ret
