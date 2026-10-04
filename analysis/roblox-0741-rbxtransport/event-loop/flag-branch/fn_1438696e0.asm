/ fcn.1438696e0();
|           0x1438696e0      sub   rsp, 0x28
|           0x1438696e4      call  fcn.1438695a0
|           0x1438696e9      mov   r8, rax
|           0x1438696ec      mov   r9, 0x5851f42d4c957f2d
|           0x1438696f6      mov   rdx, qword [rax]
|           0x1438696f9      nop   dword [rax], eax
|           ; CODE XREF from fcn.1438696e0 @ 0x143869718
|       .-> 0x143869700      mov   rcx, rdx
|       :   0x143869703      mov   rax, rdx
|       :   0x143869706      imul  rcx, r9
|       :   0x14386970a      add   rcx, 0x69                           ; 105
|       :   0x14386970e      lock  cmpxchg qword [r8], rcx
|      ,==< 0x143869713      jz    0x14386971a
|      |:   0x143869715      mov   rdx, rax
|      |`=< 0x143869718      jmp   0x143869700
|      `--> 0x14386971a      mov   r8, rdx
|           0x14386971d      mov   rax, rdx
|           0x143869720      shr   rax, 0x1b
|           0x143869724      shr   rdx, 0x3b
|           0x143869728      shr   r8, 0x2d
|           0x14386972c      mov   ecx, edx
|           0x14386972e      xor   r8d, eax
|           0x143869731      neg   ecx
|           0x143869733      and   ecx, 0x1f                           ; 31
|           0x143869736      mov   eax, r8d
|           0x143869739      shl   eax, cl
|           0x14386973b      mov   ecx, edx
|           0x14386973d      shr   r8d, cl
|           0x143869740      or    eax, r8d
|           0x143869743      add   rsp, 0x28
\           0x143869747      ret
