/ fcn.1438695a0();
|           0x1438695a0      sub   rsp, 0x28
|           0x1438695a4      mov   ecx, dword [0x14d9872d8]            ; [0x14d9872d8:4]=0
|           0x1438695aa      mov   rax, qword gs:[0x58]
|           0x1438695b3      mov   edx, 0x20                           ; 32
|           0x1438695b8      mov   rcx, qword [rax+rcx*8]
|           0x1438695bc      mov   eax, dword [rdx+rcx*1]
|           0x1438695bf      cmp   dword [0x14d7f3fd8], eax            ; [0x14d7f3fd8:4]=0
|       ,=< 0x1438695c5      jnle  0x1438695d3
|      .--> 0x1438695c7      lea   rax, qword [0x14d7f3fd0]
|      :|   0x1438695ce      add   rsp, 0x28
|      :|   0x1438695d2      ret
|      :`-> 0x1438695d3      lea   rcx, qword [0x14d7f3fd8]
|      :    0x1438695da      call  fcn.140001270
|      :    0x1438695df      cmp   dword [0x14d7f3fd8], 0xffffffff     ; [0x14d7f3fd8:4]=0
|      `==< 0x1438695e6      jnz   0x1438695c7
|           0x1438695e8      call  fcn.1438693d0
|           0x1438695ed      mov   qword [0x14d7f3fd0], rax            ; [0x14d7f3fd0:8]=0
|           0x1438695f4      lea   rcx, qword [0x14d7f3fd8]
|           0x1438695fb      call  fcn.140001350
|           0x143869600      lea   rax, qword [0x14d7f3fd0]
|           0x143869607      add   rsp, 0x28
\           0x14386960b      ret
